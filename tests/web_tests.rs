//! Contracts for the web layer's decide/report logic against a real store.

use narratarr::store::*;
use narratarr::web::{decide, queue_json, status_json};
use std::path::PathBuf;

fn temp_db(name: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("narratarr-web-{name}-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&p);
    p
}

fn ambiguous(key: &str) -> Decision {
    Decision {
        ebook_key: key.into(),
        status: DecisionStatus::Ambiguous,
        asin: Some("B000BEST".into()),
        confidence: Some(0.7),
        attempts: 1,
        next_retry: None,
        updated_at: now_epoch(),
        note: Some("Best Candidate".into()),
        ebook_title: Some("Some Book".into()),
        ebook_author: Some("Some Author".into()),
        image_url: Some("https://img/cover.jpg".into()),
        reasons: Some("no-author-metadata".into()),
    }
}

#[test]
fn accept_reject_reopen_roundtrip() {
    let path = temp_db("roundtrip");
    let store = Store::open(&path).unwrap();
    store.record(&ambiguous("k1")).unwrap();

    // accept with the stored best asin
    decide(&store, r#"{"key":"k1","action":"accept"}"#).unwrap();
    assert_eq!(
        store.get("k1").unwrap().unwrap().status,
        DecisionStatus::Matched
    );
    assert_eq!(
        store.get("k1").unwrap().unwrap().asin.as_deref(),
        Some("B000BEST")
    );

    // undo: reopen keeps identity and cover
    decide(&store, r#"{"key":"k1","action":"reopen"}"#).unwrap();
    let d = store.get("k1").unwrap().unwrap();
    assert_eq!(d.status, DecisionStatus::Ambiguous);
    assert_eq!(d.ebook_title.as_deref(), Some("Some Book"));
    assert_eq!(d.image_url.as_deref(), Some("https://img/cover.jpg"));

    // accept a DIFFERENT edition chosen from alternatives
    decide(
        &store,
        r#"{"key":"k1","action":"accept","asin":"B000OTHER"}"#,
    )
    .unwrap();
    assert_eq!(
        store.get("k1").unwrap().unwrap().asin.as_deref(),
        Some("B000OTHER")
    );

    // reject
    decide(&store, r#"{"key":"k1","action":"skip"}"#).unwrap();
    assert_eq!(
        store.get("k1").unwrap().unwrap().status,
        DecisionStatus::Skipped
    );
    let _ = std::fs::remove_file(&path);
}

#[test]
fn retry_makes_notfound_actionable_again() {
    let path = temp_db("retry");
    let store = Store::open(&path).unwrap();
    let mut d = ambiguous("k2");
    d.status = DecisionStatus::NotFound;
    d.next_retry = Some(now_epoch() + 999_999);
    store.record(&d).unwrap();
    assert!(!is_actionable(
        store.get("k2").unwrap().as_ref(),
        now_epoch()
    ));

    decide(&store, r#"{"key":"k2","action":"retry"}"#).unwrap();
    assert!(is_actionable(
        store.get("k2").unwrap().as_ref(),
        now_epoch()
    ));
    let _ = std::fs::remove_file(&path);
}

#[test]
fn queue_and_status_shapes() {
    let path = temp_db("shapes");
    let store = Store::open(&path).unwrap();
    store.record(&ambiguous("k3")).unwrap();
    store
        .set_meta("last_cycle", r#"{"at":123,"errors":0}"#)
        .unwrap();

    let q = queue_json(&store).unwrap();
    let item = &q["items"][0];
    assert_eq!(item["image_url"], "https://img/cover.jpg");
    assert_eq!(item["reasons"][0], "no-author-metadata");

    let st = status_json(&store).unwrap();
    assert_eq!(st["counts"]["ambiguous"], 1);
    assert_eq!(st["last_cycle"]["at"], 123);
    let _ = std::fs::remove_file(&path);
}
