//! Try narratarr's matcher against a directory of Calibre-style ebooks.
//!
//!     narratarr /path/to/books --limit 10 [--verbose]

use narratarr::audible;
use narratarr::listenarr::{AddOutcome, BookMetadata};
use narratarr::matcher::{match_ebook, query_title, Status};
use narratarr::opf;
use narratarr::reconcile::{self, OverrideAction};
use narratarr::store::{self, Decision, DecisionStatus, Store, NOT_FOUND_RETRY_SECS};
use narratarr::{abs, listenarr};
use std::path::PathBuf;

#[derive(Default)]
struct SyncStats {
    present: usize,
    would_add: usize,
    added: usize,
    errors: usize,
}

/// Check-then-act sync of one matched book into Listenarr. Without --apply
/// this only reports what it WOULD do.
fn sync_matched(
    client: &listenarr::Client,
    apply: bool,
    auto_search: bool,
    meta: &BookMetadata,
    stats: &mut SyncStats,
) {
    match client.exists_by_asin(&meta.asin) {
        Ok(true) => stats.present += 1,
        Ok(false) if apply => match client.add(meta, true, auto_search) {
            Ok(AddOutcome::Added) => {
                println!("++ added to listenarr: {} [{}]", meta.title, meta.asin);
                stats.added += 1;
            }
            Ok(AddOutcome::AlreadyExists) => stats.present += 1,
            Err(e) => {
                println!(
                    "!! listenarr add failed: {} [{}]: {e}",
                    meta.title, meta.asin
                );
                stats.errors += 1;
            }
        },
        Ok(false) => {
            println!("DRY-RUN would add: {} [{}]", meta.title, meta.asin);
            stats.would_add += 1;
        }
        Err(e) => {
            println!("!! listenarr lookup failed for {}: {e}", meta.asin);
            stats.errors += 1;
        }
    }
}

/// Apply the human override file: `key = ASIN` accepts a match (and syncs it
/// to Listenarr), `key = skip` permanently rejects. Idempotent by design.
fn apply_overrides(
    store: &Store,
    larr: &Option<listenarr::Client>,
    apply: bool,
    auto_search: bool,
    path: &std::path::Path,
    sync: &mut SyncStats,
) {
    let Ok(text) = std::fs::read_to_string(path) else {
        return; // no overrides file yet is a normal state
    };
    for (key, action) in reconcile::parse_overrides(&text) {
        let prior = store.get(&key).unwrap_or(None);
        let now = store::now_epoch();
        let attempts = prior.as_ref().map(|d| d.attempts).unwrap_or(0);
        match action {
            OverrideAction::Skip => {
                if prior.as_ref().map(|d| d.status) != Some(DecisionStatus::Skipped) {
                    let d = Decision {
                        ebook_key: key.clone(),
                        status: DecisionStatus::Skipped,
                        asin: None,
                        confidence: None,
                        attempts,
                        next_retry: None,
                        updated_at: now,
                        note: Some("manual override: skip".into()),
                    };
                    if store.record(&d).is_ok() {
                        println!("override: {key} = skip");
                    }
                }
            }
            OverrideAction::Asin(asin) => {
                let already = prior.as_ref().is_some_and(|d| {
                    d.status == DecisionStatus::Matched && d.asin.as_deref() == Some(asin.as_str())
                });
                if !already {
                    let d = Decision {
                        ebook_key: key.clone(),
                        status: DecisionStatus::Matched,
                        asin: Some(asin.clone()),
                        confidence: Some(1.0),
                        attempts,
                        next_retry: None,
                        updated_at: now,
                        note: Some("manual override".into()),
                    };
                    if store.record(&d).is_ok() {
                        println!("override: {key} = {asin}");
                    }
                }
                if let Some(lc) = larr {
                    let title = key.split('|').nth(1).unwrap_or(&key).to_string();
                    let author = key.split('|').next().unwrap_or("").to_string();
                    let meta = BookMetadata {
                        asin: asin.clone(),
                        title,
                        subtitle: None,
                        authors: vec![author],
                        narrators: vec![],
                        language: None,
                    };
                    sync_matched(lc, apply, auto_search, &meta, sync);
                }
            }
        }
    }
}

fn main() -> std::process::ExitCode {
    let mut args = std::env::args().skip(1);
    let mut books_dir: Option<PathBuf> = None;
    let mut limit = 10usize;
    let mut verbose = false;
    let mut lang = String::from("en");
    let mut state_path: Option<PathBuf> = None;
    let mut listenarr_url: Option<String> = None;
    let mut listenarr_key: Option<String> = None;
    let mut apply = false;
    let mut auto_search = false;
    let mut abs_url: Option<String> = None;
    let mut abs_token: Option<String> = None;
    let mut abs_library: Option<String> = None;
    let mut interval: Option<String> = None;
    let mut report_path: Option<PathBuf> = None;
    let mut overrides_path: Option<PathBuf> = None;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--limit" => limit = args.next().and_then(|v| v.parse().ok()).unwrap_or(10),
            "--verbose" => verbose = true,
            "--lang" => lang = args.next().unwrap_or_else(|| "en".into()),
            "--state" => state_path = args.next().map(PathBuf::from),
            "--listenarr" => listenarr_url = args.next(),
            "--listenarr-key" => listenarr_key = args.next(),
            "--abs" => abs_url = args.next(),
            "--abs-token" => abs_token = args.next(),
            "--abs-library" => abs_library = args.next(),
            "--apply" => apply = true,
            "--auto-search" => auto_search = true,
            "--interval" => interval = args.next(),
            "--report" => report_path = args.next().map(PathBuf::from),
            "--overrides" => overrides_path = args.next().map(PathBuf::from),
            _ => books_dir = Some(PathBuf::from(a)),
        }
    }
    let abs_mode = abs_url.is_some() || abs_token.is_some();
    if abs_mode && (abs_url.is_none() || abs_token.is_none()) {
        eprintln!("--abs and --abs-token must be given together");
        return std::process::ExitCode::FAILURE;
    }
    if !abs_mode && books_dir.is_none() {
        eprintln!(
            "usage: narratarr <books_dir> [--limit N] [--verbose] [--lang en] [--state file.db]\n       narratarr --abs <url> --abs-token <token> --abs-library <name> [...]"
        );
        return std::process::ExitCode::FAILURE;
    }
    let state = match &state_path {
        Some(p) => match Store::open(p) {
            Ok(s) => Some(s),
            Err(e) => {
                eprintln!("cannot open state db {}: {e}", p.display());
                return std::process::ExitCode::FAILURE;
            }
        },
        None => None,
    };
    let larr = match (&listenarr_url, &listenarr_key) {
        (Some(u), Some(k)) => Some(listenarr::Client::new(u, k)),
        (Some(_), None) | (None, Some(_)) => {
            eprintln!("--listenarr and --listenarr-key must be given together");
            return std::process::ExitCode::FAILURE;
        }
        (None, None) => None,
    };
    if larr.is_some() && !apply {
        println!("listenarr sync in DRY-RUN mode (pass --apply to make changes)\n");
    }
    let interval_secs = match &interval {
        Some(v) => match reconcile::parse_interval(v) {
            Some(secs) => Some(secs),
            None => {
                eprintln!("invalid --interval {v:?} (use e.g. 90s, 30m, 6h, 1d)");
                return std::process::ExitCode::FAILURE;
            }
        },
        None => None,
    };

    loop {
        let mut sync = SyncStats::default();

        if let (Some(s), Some(op)) = (&state, &overrides_path) {
            apply_overrides(s, &larr, apply, auto_search, op, &mut sync);
        }

        let (mut matched, mut ambiguous, mut not_found, mut errors) = (0, 0, 0, 0);
        let mut skipped = 0;

        let mut ebooks = if let (Some(u), Some(t)) = (&abs_url, &abs_token) {
            let client = abs::Client::new(u, t);
            let libs = match client.libraries() {
                Ok(l) => l,
                Err(e) => {
                    eprintln!("cannot list ABS libraries: {e}");
                    return std::process::ExitCode::FAILURE;
                }
            };
            let lib = abs_library
                .as_ref()
                .and_then(|want| libs.iter().find(|(_, n, _)| n.eq_ignore_ascii_case(want)));
            let Some((lib_id, lib_name, _)) = lib else {
                eprintln!("--abs-library must name one of the ABS libraries:");
                for (_, name, mt) in &libs {
                    eprintln!("  {name} ({mt})");
                }
                return std::process::ExitCode::FAILURE;
            };
            match client.ebooks(lib_id) {
                Ok(b) => {
                    println!("loaded {} items from ABS library {lib_name:?}\n", b.len());
                    b
                }
                Err(e) => {
                    eprintln!("cannot list items of ABS library {lib_name:?}: {e}");
                    return std::process::ExitCode::FAILURE;
                }
            }
        } else {
            let dir = books_dir.clone().expect("checked above");
            let mut opfs: Vec<PathBuf> = match std::fs::read_dir(&dir) {
                Ok(rd) => rd
                    .filter_map(Result::ok)
                    .map(|e| e.path())
                    .filter(|p| p.extension().is_some_and(|x| x == "opf"))
                    .collect(),
                Err(e) => {
                    eprintln!("cannot read {}: {e}", dir.display());
                    return std::process::ExitCode::FAILURE;
                }
            };
            opfs.sort();
            let mut books = Vec::new();
            for p in &opfs {
                match opf::read_opf(p) {
                    Some(b) => books.push(b),
                    None => {
                        errors += 1;
                        println!(
                            "?? unreadable opf: {}",
                            p.file_name().unwrap_or_default().to_string_lossy()
                        );
                    }
                }
            }
            books
        };
        ebooks.sort_by(|a, b| a.title.cmp(&b.title));
        ebooks.truncate(limit);

        for mut ebook in ebooks {
            // Per-book language metadata is unreliable (English books tagged "zho"
            // by scraper-sourced epubs); the user's preferred language is config.
            ebook.language = Some(lang.clone());

            // Idempotency: a prior decision means no MATCHING work — but a prior
            // matched book still gets its Listenarr presence verified (cheap local
            // GET), so state added before Listenarr was configured still syncs.
            let key = store::ebook_key(&ebook.title, &ebook.author);
            if let Some(s) = &state {
                let prior = s.get(&key).unwrap_or(None);
                if !store::is_actionable(prior.as_ref(), store::now_epoch()) {
                    if let (Some(lc), Some(d)) = (&larr, &prior) {
                        if d.status == DecisionStatus::Matched {
                            if let Some(asin) = &d.asin {
                                let meta = BookMetadata {
                                    asin: asin.clone(),
                                    title: ebook.title.clone(),
                                    subtitle: None,
                                    authors: vec![narratarr::matcher::primary_author(
                                        &ebook.author,
                                    )],
                                    narrators: vec![],
                                    language: ebook.language.clone(),
                                };
                                sync_matched(lc, apply, auto_search, &meta, &mut sync);
                            }
                        }
                    }
                    skipped += 1;
                    continue;
                }
            }

            let q = query_title(&ebook.title);
            let primary = narratarr::matcher::primary_author(&ebook.author);
            let author = if primary.is_empty() {
                None
            } else {
                Some(primary.as_str())
            };
            let candidates = match audible::search(&q, author) {
                // bad author metadata is common; retry on title alone and let the
                // scorer decide (a wrong author can then never auto-match)
                Ok(c) if c.is_empty() && author.is_some() => audible::search(&q, None),
                other => other,
            };
            let candidates = match candidates {
                Ok(c) => c,
                Err(e) => {
                    errors += 1;
                    println!("!! search failed for {:?}: {e}", ebook.title);
                    continue;
                }
            };

            let result = match_ebook(&ebook, &candidates);
            let icon = match result.status {
                Status::Matched => {
                    matched += 1;
                    "OK"
                }
                Status::Ambiguous => {
                    ambiguous += 1;
                    "??"
                }
                Status::NotFound => {
                    not_found += 1;
                    "--"
                }
            };

            if let Some(s) = &state {
                let now = store::now_epoch();
                let (status, next_retry) = match result.status {
                    Status::Matched => (DecisionStatus::Matched, None),
                    Status::Ambiguous => (DecisionStatus::Ambiguous, None),
                    Status::NotFound => {
                        (DecisionStatus::NotFound, Some(now + NOT_FOUND_RETRY_SECS))
                    }
                };
                let prior_attempts = s.get(&key).unwrap_or(None).map(|d| d.attempts).unwrap_or(0);
                let decision = Decision {
                    ebook_key: key.clone(),
                    status,
                    asin: result.best.as_ref().and_then(|b| b.asin.clone()),
                    confidence: result.best.as_ref().map(|b| b.total),
                    attempts: prior_attempts + 1,
                    next_retry,
                    updated_at: now,
                    note: result.best.as_ref().map(|b| b.title.clone()),
                };
                if let Err(e) = s.record(&decision) {
                    eprintln!("!! failed to record decision for {key}: {e}");
                }
            }

            if let (Some(lc), Status::Matched, Some(best)) = (&larr, result.status, &result.best) {
                if let Some(asin) = &best.asin {
                    // full metadata from the winning candidate; server enriches the rest
                    let cand = candidates.iter().find(|c| c.asin.as_deref() == Some(asin));
                    let meta = BookMetadata {
                        asin: asin.clone(),
                        title: cand
                            .map(|c| c.title.clone())
                            .unwrap_or_else(|| ebook.title.clone()),
                        subtitle: cand.and_then(|c| c.subtitle.clone()),
                        authors: cand.map(|c| c.authors.clone()).unwrap_or_default(),
                        narrators: cand.map(|c| c.narrators.clone()).unwrap_or_default(),
                        language: cand.and_then(|c| c.language.clone()),
                    };
                    sync_matched(lc, apply, auto_search, &meta, &mut sync);
                }
            }

            let mut line = format!(
                "{icon}  {:<48} | {:<22}",
                truncate(&ebook.title, 48),
                truncate(&ebook.author, 22)
            );
            if let Some(b) = &result.best {
                line += &format!(
                    " -> {:<40} [{}] t={:.3} a={:.3} total={:.3}",
                    truncate(&b.title, 40),
                    b.asin.as_deref().unwrap_or("-"),
                    b.title_score,
                    b.author_score,
                    b.total
                );
                if !b.penalties.is_empty() {
                    line += &format!(" ({})", b.penalties.join(","));
                }
            }
            println!("{line}");
            if verbose {
                for s in result.scored.iter().skip(1).take(3) {
                    println!(
                        "      runner-up: {:<52} total={:.3}",
                        truncate(&s.title, 52),
                        s.total
                    );
                }
            }
            std::thread::sleep(std::time::Duration::from_secs(1)); // be polite
        }

        println!(
        "\n{matched} matched / {ambiguous} ambiguous / {not_found} not found / {errors} errors / {skipped} skipped (prior decision)"
    );
        if larr.is_some() {
            println!(
                "listenarr: {} present / {} would add (dry-run) / {} added / {} errors",
                sync.present, sync.would_add, sync.added, sync.errors
            );
        }

        if let (Some(s), Some(rp)) = (&state, &report_path) {
            match s.all() {
                Ok(ds) => {
                    let md = reconcile::render_report(&ds, store::now_epoch());
                    if let Err(e) = std::fs::write(rp, md) {
                        eprintln!("!! cannot write report {}: {e}", rp.display());
                    } else {
                        println!("report written to {}", rp.display());
                    }
                }
                Err(e) => eprintln!("!! cannot read decisions for report: {e}"),
            }
        }

        match interval_secs {
            Some(secs) => {
                println!("cycle complete; next cycle in {secs}s\n");
                std::thread::sleep(std::time::Duration::from_secs(secs));
            }
            None => break,
        }
    } // loop

    std::process::ExitCode::SUCCESS
}

fn truncate(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}
