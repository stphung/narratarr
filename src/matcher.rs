//! Pure matching logic: score audiobook candidates against an ebook and
//! classify the outcome. No I/O, no state — everything here is unit-testable.
//!
//! Port of the validated Python matcher; uses the difflib crate (same
//! Ratcliff–Obershelp ratio as Python's SequenceMatcher) so the tuned
//! thresholds carry over unchanged.

use difflib::sequencematcher::SequenceMatcher;
use regex::Regex;
use std::sync::OnceLock;

pub const MATCH_THRESHOLD: f64 = 0.85; // >= this: safe to auto-add
pub const AMBIGUOUS_THRESHOLD: f64 = 0.60; // >= this: park for human review
pub const MIN_AUTHOR_SCORE: f64 = 0.60; // great title + wrong author is not a match

#[derive(Debug, Clone, Default)]
pub struct Ebook {
    pub title: String,
    pub author: String,
    pub language: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct Candidate {
    pub asin: Option<String>,
    pub title: String,
    pub subtitle: Option<String>,
    pub authors: Vec<String>,
    pub narrators: Vec<String>,
    pub format_type: Option<String>,
    pub language: Option<String>,
    pub runtime_min: Option<i64>,
    pub image_url: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Scored {
    pub asin: Option<String>,
    pub title: String,
    pub narrators: Vec<String>,
    pub image_url: Option<String>,
    pub title_score: f64,
    pub author_score: f64,
    pub total: f64,
    pub penalties: Vec<&'static str>,
    pub bonuses: Vec<&'static str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Matched,
    Ambiguous,
    NotFound,
}

#[derive(Debug)]
pub struct MatchResult {
    pub status: Status,
    pub best: Option<Scored>,
    pub scored: Vec<Scored>,
}

fn re(cell: &'static OnceLock<Regex>, pattern: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pattern).expect("static regex"))
}

// Candidate titles matching these are derivative junk, not the book.
fn junk_re() -> &'static Regex {
    static C: OnceLock<Regex> = OnceLock::new();
    re(
        &C,
        r"(?ix)\b(
            summary\s+(?:of|and\s+analysis)|
            workbook\s+for|
            study\s+guide|
            key\s+takeaways|
            analysis\s+of|
            conversation\s+starters|
            trivia|
            in\s+\d+\s+minutes
        )\b",
    )
}

// Dramatized adaptations are a different work, not a narration of the book.
// (Deliberately does NOT match "full cast": full-cast *unabridged narrations*
// are legitimate editions.)
fn dramatized_re() -> &'static Regex {
    static C: OnceLock<Regex> = OnceLock::new();
    re(
        &C,
        r"(?i)\b(dramati[sz]ed|dramati[sz]ation|graphic\s*audio|audio\s+drama)\b",
    )
}

// "Cline, Ernest - Armada" — an author name embedded at the front of the title.
fn author_prefix_in_title_re() -> &'static Regex {
    static C: OnceLock<Regex> = OnceLock::new();
    re(&C, r"^[A-Z][\w']+,\s+[A-Z][\w.' ]*?\s+-\s+")
}

// "The Lord of the Rings #01 - The Fellowship of the Ring" — keep what follows.
fn series_index_hash_re() -> &'static Regex {
    static C: OnceLock<Regex> = OnceLock::new();
    re(&C, r"#\d+\s*-\s*(.+)$")
}

// "Mistborn 06 - The Bands of Mourning" — a leading series-and-number segment.
fn series_number_prefix_re() -> &'static Regex {
    static C: OnceLock<Regex> = OnceLock::new();
    re(&C, r"^[A-Za-z][\w:' ]*?\s\d{1,2}\s+-\s+")
}

fn author_noise_re() -> &'static Regex {
    static C: OnceLock<Regex> = OnceLock::new();
    re(
        &C,
        r"(?ix)(
            \[.*?\]|
            &\s*\S*\.(?:com|net|org)\S*|
            \b(?:phd|md|esq|jr|sr|iii?)\b\.?
        )",
    )
}

fn article_suffix_re() -> &'static Regex {
    static C: OnceLock<Regex> = OnceLock::new();
    re(&C, r"(?i),\s*(the|a|an)$")
}

fn year_suffix_re() -> &'static Regex {
    static C: OnceLock<Regex> = OnceLock::new();
    re(&C, r"\s*\(\d{4}\)\s*$")
}

fn series_hint_re() -> &'static Regex {
    static C: OnceLock<Regex> = OnceLock::new();
    re(&C, r"(?i)(\ba\s+.{2,40}?\s+novel\b|\bbook\s+\d+\b|#\d+)")
}

fn non_word_keep_colon_re() -> &'static Regex {
    static C: OnceLock<Regex> = OnceLock::new();
    re(&C, r"[^\w\s:]")
}

fn non_word_re() -> &'static Regex {
    static C: OnceLock<Regex> = OnceLock::new();
    re(&C, r"[^\w\s]")
}

fn spaces_re() -> &'static Regex {
    static C: OnceLock<Regex> = OnceLock::new();
    re(&C, r"\s+")
}

fn paren_tail_re() -> &'static Regex {
    static C: OnceLock<Regex> = OnceLock::new();
    re(&C, r"\s*\([^)]*\)\s*$")
}

fn squeeze(s: &str) -> String {
    spaces_re().replace_all(s.trim(), " ").to_string()
}

/// Lowercased title with articles, years, punctuation and filename artifacts removed.
pub fn normalize_title(raw: &str) -> String {
    let t = raw.trim();
    let t = year_suffix_re().replace(t, "");
    let t = article_suffix_re().replace(&t, ""); // "Alloy of Law, The" -> "Alloy of Law"
    let t = t.replace('_', ":").to_lowercase(); // filename-safe colon back to a colon
    let t = non_word_keep_colon_re().replace_all(&t, " ");
    squeeze(&t)
}

/// The part before any subtitle: "alloy of law: a mistborn novel" -> "alloy of law".
pub fn base_title(normalized: &str) -> String {
    let head = normalized.split(':').next().unwrap_or(normalized).trim();
    let head = squeeze(&series_hint_re().replace_all(head, ""));
    if head.is_empty() {
        normalized.to_string()
    } else {
        head
    }
}

/// Light cleanup for a SEARCH QUERY: fix filename artifacts but keep
/// punctuation — search engines want "Abaddon's Gate", not "abaddon s gate".
pub fn query_title(raw: &str) -> String {
    let t = raw.trim();
    let t = year_suffix_re().replace(t, "");
    let t = article_suffix_re().replace(&t, "");
    let t = t.replace('_', ":");
    let t = t.split(':').next().unwrap_or(&t).trim().to_string();
    let t = paren_tail_re().replace(&t, "").trim().to_string(); // "(wow-4)"-style tags
    if t.is_empty() {
        raw.trim().to_string()
    } else {
        t
    }
}

/// Filename-style junk some libraries embed in the TITLE field itself.
/// Applied once at ingestion (see opf.rs), so scoring and querying both see
/// the cleaned title.
pub fn preclean_title(raw: &str) -> String {
    let t = raw.trim();
    let t = author_prefix_in_title_re().replace(t, "").to_string();
    let t = match series_index_hash_re().captures(&t) {
        Some(c) => c.get(1).map(|m| m.as_str().to_string()).unwrap_or(t),
        None => t,
    };
    let t = series_number_prefix_re().replace(&t, "").trim().to_string();
    if t.is_empty() {
        raw.trim().to_string()
    } else {
        t
    }
}

/// First author of a possibly semicolon-separated list: "A;B;C" -> "A".
pub fn primary_author(raw: &str) -> String {
    raw.split(';')
        .map(str::trim)
        .find(|s| !s.is_empty())
        .unwrap_or("")
        .to_string()
}

/// "Bryson, Bill" -> "bill bryson"; strips credentials and bracket/site junk.
pub fn normalize_author(raw: &str) -> String {
    let raw = primary_author(raw);
    let a = author_noise_re().replace_all(&raw, " ");
    let a = a.trim().trim_matches('&').trim().to_string();
    let a = {
        let parts: Vec<&str> = a
            .split(',')
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .collect();
        if parts.len() == 2 && !parts[0].contains(' ') {
            format!("{} {}", parts[1], parts[0]) // "Last, First" -> "First Last"
        } else {
            a
        }
    };
    let a = a.to_lowercase();
    let a = non_word_re().replace_all(&a, " ");
    squeeze(&a)
}

fn ratio(a: &str, b: &str) -> f64 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    SequenceMatcher::new(a, b).ratio() as f64
}

/// Similarity that tolerates subtitle asymmetry between editions.
pub fn title_score(ebook_title: &str, cand_title: &str, cand_subtitle: Option<&str>) -> f64 {
    let e_full = normalize_title(ebook_title);
    let c_full = match cand_subtitle {
        Some(sub) if !sub.is_empty() => normalize_title(&format!("{cand_title}: {sub}")),
        _ => normalize_title(cand_title),
    };
    let e_base = base_title(&e_full);
    let c_base = base_title(&normalize_title(cand_title));

    let mut score: f64 = [
        ratio(&e_full, &c_full),
        ratio(&e_base, &c_base),
        ratio(&e_base, &c_full), // one side carries a subtitle the other lacks
        ratio(&e_full, &c_base),
    ]
    .into_iter()
    .fold(0.0, f64::max);

    if !e_base.is_empty() && e_base == c_base {
        score = score.max(0.95); // exact base-title equality is a strong signal
    } else {
        // same signal when the bases differ only by stopwords/edition words
        // ("Start with Why" vs "Start with Why 15th Anniversary Edition")
        let (ew, cw) = (content_words(&e_base), content_words(&c_base));
        if !ew.is_empty() && ew == cw {
            score = score.max(0.95);
        }
    }
    score
}

/// Best similarity against any credited author; last-name agreement is weighted.
pub fn author_score(ebook_author: &str, cand_authors: &[String]) -> f64 {
    let e = normalize_author(ebook_author);
    if e.is_empty() || cand_authors.is_empty() {
        return 0.0;
    }
    let e_last = e.split_whitespace().last().unwrap_or_default();
    let mut best: f64 = 0.0;
    for raw in cand_authors {
        let c = normalize_author(raw);
        if c.is_empty() {
            continue;
        }
        let mut s = ratio(&e, &c);
        if c.split_whitespace().last() == Some(e_last) {
            s = s.max(0.75 + 0.25 * s); // same surname rescues initials/diacritics
        }
        best = best.max(s);
    }
    best
}

const STOPWORDS: &[&str] = &[
    "the", "a", "an", "of", "and", "in", "on", "at", "to", "for", "with", "s",
];

// Words that describe an EDITION rather than identify a work. A candidate
// differing only by these is the same book repackaged, not a different book.
const EDITION_WORDS: &[&str] = &[
    "edition",
    "revised",
    "updated",
    "expanded",
    "anniversary",
    "complete",
    "unabridged",
    "new",
    "special",
];

fn is_ordinal(w: &str) -> bool {
    // "10th", "2nd", "15th" — edition ordinals
    w.chars().next().is_some_and(|c| c.is_ascii_digit())
        && (w.ends_with("st") || w.ends_with("nd") || w.ends_with("rd") || w.ends_with("th"))
}

fn content_words(s: &str) -> std::collections::HashSet<String> {
    s.split_whitespace()
        .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()))
        .filter(|w| {
            !w.is_empty() && !STOPWORDS.contains(w) && !EDITION_WORDS.contains(w) && !is_ordinal(w)
        })
        .map(str::to_string)
        .collect()
}

/// Score one candidate. Total is clamped to [0, 1].
pub fn score_candidate(ebook: &Ebook, cand: &Candidate) -> Scored {
    let mut t = title_score(&ebook.title, &cand.title, cand.subtitle.as_deref());
    let a = author_score(&ebook.author, &cand.authors);

    // A candidate whose BASE title carries content words the ebook title lacks
    // is likely a different work ("The Effective HIRING Manager") or a branded
    // edition ("SLY FLOURISH'S Lazy Dungeon Master"). Sequence similarity stays
    // deceptively high in these cases; dampen it so they land in review, not
    // auto-add. Precision over recall.
    let extra_penalty = {
        let cand_base = base_title(&normalize_title(&cand.title));
        let ebook_full = normalize_title(&ebook.title);
        let extra: Vec<String> = content_words(&cand_base)
            .difference(&content_words(&ebook_full))
            .cloned()
            .collect();
        !extra.is_empty()
    };
    if extra_penalty {
        t *= 0.7;
    }

    let mut total = 0.45 * t + 0.45 * a;
    let mut penalties = Vec::new();
    let mut bonuses = Vec::new();

    let full_cand_title = format!("{} {}", cand.title, cand.subtitle.as_deref().unwrap_or(""));
    if junk_re().is_match(&full_cand_title) {
        total -= 0.60;
        penalties.push("derivative-work");
    }
    if dramatized_re().is_match(&full_cand_title) {
        total -= 0.20;
        penalties.push("dramatized");
    }
    if extra_penalty {
        penalties.push("title-extra-words");
    }
    match cand
        .format_type
        .as_deref()
        .map(str::to_lowercase)
        .as_deref()
    {
        Some("abridged") => {
            total -= 0.15;
            penalties.push("abridged");
        }
        Some("unabridged") => {
            total += 0.05;
            bonuses.push("unabridged");
        }
        _ => {}
    }
    let lang2 = |s: &Option<String>| {
        s.as_deref()
            .map(|v| v.to_lowercase().chars().take(2).collect::<String>())
            .filter(|v| !v.is_empty())
    };
    if let (Some(e), Some(c)) = (lang2(&ebook.language), lang2(&cand.language)) {
        if e == c {
            total += 0.05;
            bonuses.push("language");
        } else {
            total -= 0.30;
            penalties.push("language-mismatch");
        }
    }

    Scored {
        asin: cand.asin.clone(),
        title: cand.title.clone(),
        narrators: cand.narrators.clone(),
        image_url: cand.image_url.clone(),
        title_score: t,
        author_score: a,
        total: total.clamp(0.0, 1.0),
        penalties,
        bonuses,
    }
}

/// Classify an ebook against its candidate list.
pub fn match_ebook(ebook: &Ebook, candidates: &[Candidate]) -> MatchResult {
    let mut scored: Vec<Scored> = candidates
        .iter()
        .map(|c| score_candidate(ebook, c))
        .collect();
    scored.sort_by(|x, y| {
        y.total
            .partial_cmp(&x.total)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let best = scored
        .iter()
        .find(|s| !s.penalties.contains(&"derivative-work"))
        .cloned();

    // An ebook with NO author metadata can never confirm a match, but a very
    // strong title deserves human review rather than silent refusal.
    let author_unknown = normalize_author(&ebook.author).is_empty();
    let status = match &best {
        None => Status::NotFound,
        Some(b) if b.total >= MATCH_THRESHOLD && b.author_score >= MIN_AUTHOR_SCORE => {
            Status::Matched
        }
        Some(b) if b.total >= AMBIGUOUS_THRESHOLD => Status::Ambiguous,
        Some(b) if author_unknown && b.title_score >= 0.9 => Status::Ambiguous,
        Some(_) => Status::NotFound,
    };

    MatchResult {
        status,
        best,
        scored,
    }
}
