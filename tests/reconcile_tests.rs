//! Contracts for the reconcile-loop plumbing.

use narratarr::reconcile::*;
use narratarr::store::{Decision, DecisionStatus};

#[test]
fn interval_parsing() {
    assert_eq!(parse_interval("90s"), Some(90));
    assert_eq!(parse_interval("30m"), Some(1800));
    assert_eq!(parse_interval("6h"), Some(21600));
    assert_eq!(parse_interval("1d"), Some(86400));
    assert_eq!(parse_interval("6x"), None);
    assert_eq!(parse_interval(""), None);
}

#[test]
fn overrides_parsing() {
    let text = "\n# comment\nbrandon sanderson|mistborn: secret history = B01DPMS8JC\n\
                aaron rosenberg|beyond the dark portal = SKIP   # reject\nbadline\n= B0X\n";
    let o = parse_overrides(text);
    assert_eq!(o.len(), 2);
    assert_eq!(o[0].1, OverrideAction::Asin("B01DPMS8JC".into()));
    assert_eq!(o[1].1, OverrideAction::Skip);
}

#[test]
fn report_contains_paste_ready_override_lines() {
    let d = Decision {
        ebook_key: "matt dinniman|carl s doomsday scenario".into(),
        status: DecisionStatus::Ambiguous,
        asin: Some("B0934GTSGT".into()),
        confidence: Some(0.63),
        attempts: 1,
        next_retry: None,
        updated_at: 0,
        note: Some("Carl's Doomsday Scenario".into()),
        ebook_title: None,
        ebook_author: None,
    };
    let md = render_report(&[d], 0);
    assert!(md.contains("Needs review"));
    assert!(md.contains("`matt dinniman|carl s doomsday scenario = B0934GTSGT`"));
    assert!(md.contains("= skip`"));
}
