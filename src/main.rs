//! Try shelfspoken's matcher against a directory of Calibre-style ebooks.
//!
//!     shelfspoken /path/to/books --limit 10 [--verbose]

use shelfspoken::audible;
use shelfspoken::matcher::{match_ebook, query_title, Status};
use shelfspoken::opf;
use std::path::PathBuf;

fn main() -> std::process::ExitCode {
    let mut args = std::env::args().skip(1);
    let mut books_dir: Option<PathBuf> = None;
    let mut limit = 10usize;
    let mut verbose = false;
    let mut lang = String::from("en");
    while let Some(a) = args.next() {
        match a.as_str() {
            "--limit" => limit = args.next().and_then(|v| v.parse().ok()).unwrap_or(10),
            "--verbose" => verbose = true,
            "--lang" => lang = args.next().unwrap_or_else(|| "en".into()),
            _ => books_dir = Some(PathBuf::from(a)),
        }
    }
    let Some(dir) = books_dir else {
        eprintln!("usage: shelfspoken <books_dir> [--limit N] [--verbose]");
        return std::process::ExitCode::FAILURE;
    };

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
    opfs.truncate(limit);

    let (mut matched, mut ambiguous, mut not_found, mut errors) = (0, 0, 0, 0);
    for opf_path in &opfs {
        let Some(mut ebook) = opf::read_opf(opf_path) else {
            errors += 1;
            println!("?? unreadable opf: {}", opf_path.file_name().unwrap_or_default().to_string_lossy());
            continue;
        };

        // Per-book language metadata is unreliable (English books tagged "zho"
        // by scraper-sourced epubs); the user's preferred language is config.
        ebook.language = Some(lang.clone());

        let q = query_title(&ebook.title);
        let primary = shelfspoken::matcher::primary_author(&ebook.author);
        let author = if primary.is_empty() { None } else { Some(primary.as_str()) };
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
                println!("      runner-up: {:<52} total={:.3}", truncate(&s.title, 52), s.total);
            }
        }
        std::thread::sleep(std::time::Duration::from_secs(1)); // be polite
    }

    println!("\n{matched} matched / {ambiguous} ambiguous / {not_found} not found / {errors} errors");
    std::process::ExitCode::SUCCESS
}

fn truncate(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}
