//! Fixture tests pinning the verdicts validated against a real 25-book
//! Calibre library on 2026-08-20. If a heuristic tweak changes any of these,
//! it must be a conscious decision.

use narratarr::matcher::*;

fn ebook(title: &str, author: &str) -> Ebook {
    Ebook { title: title.into(), author: author.into(), language: Some("en".into()) }
}

fn cand(title: &str, subtitle: Option<&str>, authors: &[&str]) -> Candidate {
    Candidate {
        asin: Some("B000TEST".into()),
        title: title.into(),
        subtitle: subtitle.map(Into::into),
        authors: authors.iter().map(|s| s.to_string()).collect(),
        format_type: Some("unabridged".into()),
        language: Some("english".into()),
        ..Default::default()
    }
}

// ── normalizers, against the library's real filename mess ───────────────

#[test]
fn title_filename_artifacts() {
    assert_eq!(normalize_title("Alloy of Law_ A Mistborn Novel, The"), "alloy of law: a mistborn novel");
    assert_eq!(base_title(&normalize_title("Alloy of Law_ A Mistborn Novel, The")), "alloy of law");
    assert_eq!(normalize_title("a Short History Of Nearly Everything (2010)"), "a short history of nearly everything");
}

#[test]
fn author_reversed_and_junk() {
    assert_eq!(normalize_author("Bryson, Bill"), "bill bryson");
    assert_eq!(normalize_author("Skiena, Steven S. [Skiena, Steven S.] & chenjin5.com"), "steven s skiena");
    assert_eq!(normalize_author("Forsgren PhD"), "forsgren");
}

#[test]
fn query_title_keeps_punctuation_strips_tags() {
    // normalize for comparison, never for queries: the apostrophe bug
    assert_eq!(query_title("Abaddon's Gate"), "Abaddon's Gate");
    assert_eq!(query_title("Beyond the Dark Portal (wow-4)"), "Beyond the Dark Portal");
    assert_eq!(query_title("Alloy of Law_ A Mistborn Novel, The"), "Alloy of Law");
}

// ── classification verdicts ─────────────────────────────────────────────

#[test]
fn exact_match_is_matched() {
    let r = match_ebook(
        &ebook("Animal Farm", "George Orwell"),
        &[cand("Animal Farm", None, &["George Orwell"])],
    );
    assert_eq!(r.status, Status::Matched);
}

#[test]
fn subtitle_asymmetry_still_matches() {
    // ebook: "Accelerate" / audiobook: "Accelerate: Building and Scaling..."
    let r = match_ebook(
        &ebook("Accelerate", "Forsgren PhD"),
        &[cand(
            "Accelerate",
            Some("Building and Scaling High Performing Technology Organizations"),
            &["Nicole Forsgren PhD", "Jez Humble", "Gene Kim"],
        )],
    );
    assert_eq!(r.status, Status::Matched);
}

#[test]
fn summary_junk_is_rejected() {
    let r = match_ebook(
        &ebook("Astrophysics for People in a Hurry", "Neil deGrasse Tyson"),
        &[cand(
            "Summary of Astrophysics for People in a Hurry",
            None,
            &["Goldmine Reads"],
        )],
    );
    assert_eq!(r.status, Status::NotFound);
    assert!(r.best.is_none()); // junk is not even eligible as best
}

#[test]
fn perfect_title_with_bogus_author_is_ambiguous_not_matched() {
    // "Carl's Doomsday Scenario" credited to uploader handle "DoctorHepa"
    let r = match_ebook(
        &ebook("Carl's Doomsday Scenario", "DoctorHepa"),
        &[cand("Carl's Doomsday Scenario", None, &["Matt Dinniman"])],
    );
    assert_eq!(r.status, Status::Ambiguous);
    assert!(r.best.unwrap().author_score < MIN_AUTHOR_SCORE);
}

#[test]
fn franchise_prefixed_title_is_ambiguous() {
    // ebook "Beyond the Dark Portal" vs "World of Warcraft: Beyond the Dark Portal"
    let r = match_ebook(
        &ebook("Beyond the Dark Portal (wow-4)", "Aaron Rosenberg"),
        &[cand(
            "World of Warcraft",
            Some("Beyond the Dark Portal"),
            &["Aaron Rosenberg", "Christie Golden"],
        )],
    );
    assert_eq!(r.status, Status::Ambiguous);
}

#[test]
fn wrong_language_is_penalized() {
    let mut c = cand("Abaddon's Gate - La fuga", None, &["James S. A. Corey"]);
    c.language = Some("italian".into());
    let r = match_ebook(&ebook("Abaddon's Gate", "James S. A. Corey"), &[c]);
    assert_ne!(r.status, Status::Matched);
}

#[test]
fn reversed_author_still_confirms() {
    let r = match_ebook(
        &ebook("Cibola Burn", "Corey, James S. A"),
        &[cand("Cibola Burn", None, &["James S. A. Corey"])],
    );
    assert_eq!(r.status, Status::Matched);
}

#[test]
fn abridged_is_outranked_by_unabridged() {
    let mut abridged = cand("Dune", None, &["Frank Herbert"]);
    abridged.format_type = Some("abridged".into());
    abridged.asin = Some("ABRIDGED".into());
    let unabridged = cand("Dune", None, &["Frank Herbert"]);
    let r = match_ebook(&ebook("Dune", "Frank Herbert"), &[abridged, unabridged]);
    assert_eq!(r.status, Status::Matched);
    assert_eq!(r.best.unwrap().asin.as_deref(), Some("B000TEST"));
}

// ── fixes from the 236-book full-library sweep (2026-08-20) ─────────────

#[test]
fn dramatized_adaptation_is_not_auto_matched() {
    // the one false positive of the full sweep: Gatsby (Dramatized) at 0.855
    let r = match_ebook(
        &ebook("The Great Gatsby (\"Global Classics\")", "F. Scott Fitzgerald"),
        &[cand("The Great Gatsby (Dramatized)", None, &["F. Scott Fitzgerald"])],
    );
    assert_ne!(r.status, Status::Matched);
}

#[test]
fn semicolon_author_lists_use_first_author() {
    assert_eq!(normalize_author("Horstman, Mark;Braun, Michael"), "mark horstman");
    assert_eq!(normalize_author("Max Brooks;"), "max brooks");
    let r = match_ebook(
        &ebook("The Effective Manager", "Horstman, Mark;Braun, Michael"),
        &[cand("The Effective Manager", None, &["Mark Horstman"])],
    );
    assert_eq!(r.status, Status::Matched);
}

#[test]
fn preclean_strips_filename_junk_from_titles() {
    assert_eq!(preclean_title("Cline, Ernest - Armada: A Novel"), "Armada: A Novel");
    assert_eq!(
        preclean_title("Sanderson, Brandon - Mistborn 06 - The Bands of Mourning"),
        "The Bands of Mourning"
    );
    assert_eq!(
        preclean_title("The Lord of the Rings #01 - The Fellowship of the Ring"),
        "The Fellowship of the Ring"
    );
    // must NOT touch legitimate hyphenated titles — this refusal was correct
    assert_eq!(
        preclean_title("Astrophysics for People in a Hurry - Summarized for Busy People"),
        "Astrophysics for People in a Hurry - Summarized for Busy People"
    );
}

#[test]
fn adjacent_title_same_author_is_not_auto_matched() {
    // "The Effective HIRING Manager" is a different Horstman book; sequence
    // similarity alone scores it deceptively well
    let r = match_ebook(
        &ebook("The Effective Manager", "Horstman, Mark;Braun, Kate"),
        &[cand("The Effective Hiring Manager", None, &["Mark Horstman"])],
    );
    assert_ne!(r.status, Status::Matched);
    assert!(r.best.unwrap().penalties.contains(&"title-extra-words"));
}

#[test]
fn extra_words_rule_spares_subtitle_only_differences() {
    // candidate base == ebook base; extra words live in the SUBTITLE, which
    // editions legitimately disagree on
    let r = match_ebook(
        &ebook("Accelerate", "Nicole Forsgren"),
        &[cand(
            "Accelerate",
            Some("Building and Scaling High Performing Technology Organizations"),
            &["Nicole Forsgren PhD"],
        )],
    );
    assert_eq!(r.status, Status::Matched);
}

#[test]
fn edition_suffix_words_do_not_block_auto_match() {
    // "15th Anniversary Edition" is the same book repackaged, not a different work
    let r = match_ebook(
        &ebook("Start with Why: How Great Leaders Inspire Everyone to Take Action", "Simon Sinek"),
        &[cand("Start with Why 15th Anniversary Edition", None, &["Simon Sinek"])],
    );
    assert_eq!(r.status, Status::Matched);
    let r = match_ebook(
        &ebook("Multipliers: How the Best Leaders Make Everyone Smarter", "Liz Wiseman"),
        &[cand("Multipliers, Revised and Updated", Some("How the Best Leaders Make Everyone Smarter"), &["Liz Wiseman"])],
    );
    assert_eq!(r.status, Status::Matched);
}
