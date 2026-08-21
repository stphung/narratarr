//! The decision store's contract: idempotent upserts, retry backoff, and
//! survival across reopen — the properties the reconcile loop leans on.

use narratarr::store::*;
use std::path::PathBuf;

fn temp_db(name: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("narratarr-test-{name}-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&p);
    p
}

fn decision(key: &str, status: DecisionStatus, next_retry: Option<i64>) -> Decision {
    Decision {
        ebook_key: key.into(),
        status,
        asin: Some("B000TEST".into()),
        confidence: Some(0.9),
        attempts: 1,
        next_retry,
        updated_at: now_epoch(),
        note: None,
        ebook_title: None,
        ebook_author: None,
    }
}

#[test]
fn record_get_roundtrip_and_upsert() {
    let path = temp_db("roundtrip");
    let store = Store::open(&path).unwrap();

    let key = ebook_key("The Way of Kings", "Sanderson, Brandon");
    store
        .record(&decision(&key, DecisionStatus::Ambiguous, None))
        .unwrap();
    let got = store.get(&key).unwrap().unwrap();
    assert_eq!(got.status, DecisionStatus::Ambiguous);

    // upsert: same key, new verdict — last write wins, no duplicate row
    store
        .record(&decision(&key, DecisionStatus::Matched, None))
        .unwrap();
    assert_eq!(store.all().unwrap().len(), 1);
    assert_eq!(
        store.get(&key).unwrap().unwrap().status,
        DecisionStatus::Matched
    );
    let _ = std::fs::remove_file(&path);
}

#[test]
fn state_survives_reopen() {
    let path = temp_db("reopen");
    {
        let store = Store::open(&path).unwrap();
        store
            .record(&decision("k", DecisionStatus::Matched, None))
            .unwrap();
    }
    let store = Store::open(&path).unwrap();
    assert_eq!(
        store.get("k").unwrap().unwrap().status,
        DecisionStatus::Matched
    );
    let _ = std::fs::remove_file(&path);
}

#[test]
fn actionability_rules() {
    let now = now_epoch();
    // new book: work on it
    assert!(is_actionable(None, now));
    // matched or awaiting human: never re-process automatically
    assert!(!is_actionable(
        Some(&decision("k", DecisionStatus::Matched, None)),
        now
    ));
    assert!(!is_actionable(
        Some(&decision("k", DecisionStatus::Ambiguous, None)),
        now
    ));
    assert!(!is_actionable(
        Some(&decision("k", DecisionStatus::Skipped, None)),
        now
    ));
    // not_found: honor the backoff timer
    assert!(!is_actionable(
        Some(&decision("k", DecisionStatus::NotFound, Some(now + 1000))),
        now
    ));
    assert!(is_actionable(
        Some(&decision("k", DecisionStatus::NotFound, Some(now - 1))),
        now
    ));
    assert!(is_actionable(
        Some(&decision("k", DecisionStatus::NotFound, None)),
        now
    ));
}

#[test]
fn ebook_key_is_stable_across_metadata_styles() {
    // the same book expressed two ways must produce the same identity
    assert_eq!(
        ebook_key("The Way of Kings", "Sanderson, Brandon"),
        ebook_key("The Way of Kings", "Brandon Sanderson"),
    );
}
