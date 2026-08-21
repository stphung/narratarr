//! The decision store — narratarr's ONLY owned state.
//!
//! One SQLite table records match *decisions* (ebook → asin/status), plus the
//! retry bookkeeping for books whose audiobook doesn't exist yet. Every other
//! fact (what ebooks exist, what's monitored) is queried live from the system
//! that owns it, so this database can be deleted and fully regenerated —
//! losing only manual overrides and retry timers.

use crate::matcher::{normalize_author, normalize_title};
use rusqlite::{params, Connection};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

/// How long to wait before re-searching a book whose audiobook wasn't found.
/// Availability changes on the scale of months, and indexer goodwill is finite.
pub const NOT_FOUND_RETRY_SECS: i64 = 30 * 24 * 3600;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecisionStatus {
    Matched,
    Ambiguous,
    NotFound,
    Skipped,
}

impl DecisionStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Matched => "matched",
            Self::Ambiguous => "ambiguous",
            Self::NotFound => "not_found",
            Self::Skipped => "skipped",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "matched" => Some(Self::Matched),
            "ambiguous" => Some(Self::Ambiguous),
            "not_found" => Some(Self::NotFound),
            "skipped" => Some(Self::Skipped),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Decision {
    pub ebook_key: String,
    pub status: DecisionStatus,
    pub asin: Option<String>,
    pub confidence: Option<f64>,
    pub attempts: i64,
    pub next_retry: Option<i64>, // epoch seconds; None = never retry automatically
    pub updated_at: i64,         // epoch seconds
    pub note: Option<String>,    // human context, e.g. the best candidate's title
    pub ebook_title: Option<String>, // the source ebook, for re-search and display
    pub ebook_author: Option<String>,
}

/// Stable identity for an ebook across cycles: normalized author + title.
/// (When the Audiobookshelf client lands, its item id becomes the preferred
/// key; this derivation stays as the fallback for directory mode.)
pub fn ebook_key(title: &str, author: &str) -> String {
    format!("{}|{}", normalize_author(author), normalize_title(title))
}

pub fn now_epoch() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub struct Store {
    conn: Connection,
}

impl Store {
    pub fn open(path: &Path) -> rusqlite::Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             CREATE TABLE IF NOT EXISTS decisions (
                 ebook_key   TEXT PRIMARY KEY,
                 status      TEXT NOT NULL,
                 asin        TEXT,
                 confidence  REAL,
                 attempts    INTEGER NOT NULL DEFAULT 0,
                 next_retry  INTEGER,
                 updated_at  INTEGER NOT NULL,
                 note        TEXT,
                 ebook_title TEXT,
                 ebook_author TEXT
             );",
        )?;
        // migrations for stores created before these columns existed
        let _ = conn.execute("ALTER TABLE decisions ADD COLUMN note TEXT", []);
        let _ = conn.execute("ALTER TABLE decisions ADD COLUMN ebook_title TEXT", []);
        let _ = conn.execute("ALTER TABLE decisions ADD COLUMN ebook_author TEXT", []);
        Ok(Self { conn })
    }

    pub fn get(&self, key: &str) -> rusqlite::Result<Option<Decision>> {
        self.conn
            .query_row(
                "SELECT ebook_key, status, asin, confidence, attempts, next_retry, updated_at, note, ebook_title, ebook_author
                 FROM decisions WHERE ebook_key = ?1",
                params![key],
                row_to_decision,
            )
            .map(Some)
            .or_else(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => Ok(None),
                other => Err(other),
            })
    }

    /// Insert or replace the decision for this ebook (last write wins).
    pub fn record(&self, d: &Decision) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT INTO decisions (ebook_key, status, asin, confidence, attempts, next_retry, updated_at, note, ebook_title, ebook_author)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
             ON CONFLICT(ebook_key) DO UPDATE SET
                 status = excluded.status,
                 asin = excluded.asin,
                 confidence = excluded.confidence,
                 attempts = excluded.attempts,
                 next_retry = excluded.next_retry,
                 updated_at = excluded.updated_at,
                 note = excluded.note,
                 ebook_title = excluded.ebook_title,
                 ebook_author = excluded.ebook_author",
            params![
                d.ebook_key,
                d.status.as_str(),
                d.asin,
                d.confidence,
                d.attempts,
                d.next_retry,
                d.updated_at,
                d.note,
                d.ebook_title,
                d.ebook_author
            ],
        )?;
        Ok(())
    }

    pub fn all(&self) -> rusqlite::Result<Vec<Decision>> {
        let mut stmt = self.conn.prepare(
            "SELECT ebook_key, status, asin, confidence, attempts, next_retry, updated_at, note, ebook_title, ebook_author
             FROM decisions ORDER BY ebook_key",
        )?;
        let rows = stmt.query_map([], row_to_decision)?;
        rows.collect()
    }
}

fn row_to_decision(row: &rusqlite::Row<'_>) -> rusqlite::Result<Decision> {
    let status_text: String = row.get(1)?;
    Ok(Decision {
        ebook_key: row.get(0)?,
        status: DecisionStatus::parse(&status_text).unwrap_or(DecisionStatus::Skipped),
        asin: row.get(2)?,
        confidence: row.get(3)?,
        attempts: row.get(4)?,
        next_retry: row.get(5)?,
        updated_at: row.get(6)?,
        note: row.get(7)?,
        ebook_title: row.get(8)?,
        ebook_author: row.get(9)?,
    })
}

/// Should this cycle do work on a book, given its prior decision?
///
/// - no prior decision      → yes, it's new
/// - matched                → no, done forever (Listenarr owns it now)
/// - ambiguous / skipped    → no, waiting on a human (override file, later)
/// - not_found              → only once its retry backoff expires
pub fn is_actionable(prior: Option<&Decision>, now: i64) -> bool {
    match prior {
        None => true,
        Some(d) => match d.status {
            DecisionStatus::Matched | DecisionStatus::Ambiguous | DecisionStatus::Skipped => false,
            DecisionStatus::NotFound => d.next_retry.is_none_or(|t| now >= t),
        },
    }
}
