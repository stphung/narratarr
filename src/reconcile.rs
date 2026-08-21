//! Reconcile-loop plumbing: interval parsing, the human override file, and
//! the review report. All pure functions — the loop itself lives in main.

use crate::store::{Decision, DecisionStatus};

/// "90s" / "30m" / "6h" / "1d" → seconds.
pub fn parse_interval(s: &str) -> Option<u64> {
    let s = s.trim();
    let (num, unit) = s.split_at(s.len().checked_sub(1)?);
    let n: u64 = num.parse().ok()?;
    match unit {
        "s" => Some(n),
        "m" => Some(n * 60),
        "h" => Some(n * 3600),
        "d" => Some(n * 86400),
        _ => None,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OverrideAction {
    Asin(String),
    Skip,
}

/// One override per line: `<ebook_key> = <ASIN>` or `<ebook_key> = skip`.
/// `#` starts a comment. Malformed lines are ignored (reported by caller via
/// the count difference if it cares).
pub fn parse_overrides(text: &str) -> Vec<(String, OverrideAction)> {
    text.lines()
        .filter_map(|line| {
            let line = line.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                return None;
            }
            let (key, value) = line.rsplit_once('=')?;
            let (key, value) = (key.trim(), value.trim());
            if key.is_empty() || value.is_empty() {
                return None;
            }
            let action = if value.eq_ignore_ascii_case("skip") {
                OverrideAction::Skip
            } else {
                OverrideAction::Asin(value.to_string())
            };
            Some((key.to_string(), action))
        })
        .collect()
}

/// The cycle's human-facing output: what needs review (with ready-to-paste
/// override lines) and what wasn't found.
pub fn render_report(decisions: &[Decision], now: i64) -> String {
    let mut md = String::from("# narratarr review report\n\n");

    let ambiguous: Vec<_> = decisions
        .iter()
        .filter(|d| d.status == DecisionStatus::Ambiguous)
        .collect();
    let not_found: Vec<_> = decisions
        .iter()
        .filter(|d| d.status == DecisionStatus::NotFound)
        .collect();
    let matched = decisions
        .iter()
        .filter(|d| d.status == DecisionStatus::Matched)
        .count();

    md.push_str(&format!(
        "{matched} matched · {} needing review · {} not found\n\n",
        ambiguous.len(),
        not_found.len()
    ));

    if !ambiguous.is_empty() {
        md.push_str("## Needs review\n\nAccept a match by adding the line to your overrides file; reject with `= skip`.\n\n");
        for d in &ambiguous {
            let cand = d.note.as_deref().unwrap_or("(unknown candidate)");
            let asin = d.asin.as_deref().unwrap_or("?");
            let conf = d.confidence.unwrap_or(0.0);
            md.push_str(&format!("- **{}**\n  best candidate: {cand} [{asin}] (confidence {conf:.2})\n  accept: `{} = {asin}`\n  reject: `{} = skip`\n\n", d.ebook_key, d.ebook_key, d.ebook_key));
        }
    }

    if !not_found.is_empty() {
        md.push_str(
            "## Not found\n\nRetried automatically; add an override if you know the ASIN.\n\n",
        );
        for d in &not_found {
            let days = d
                .next_retry
                .map(|t| ((t - now).max(0)) / 86400)
                .unwrap_or(0);
            md.push_str(&format!(
                "- {} (attempt {}, retry in {days}d)\n",
                d.ebook_key, d.attempts
            ));
        }
    }

    md
}
