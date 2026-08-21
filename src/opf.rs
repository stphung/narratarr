//! Read title/author/language from Calibre .opf sidecar files.
//!
//! Deliberately regex-based rather than a full XML parser: Calibre OPF files
//! are machine-generated and regular, and this keeps the dependency tree flat.

use crate::matcher::Ebook;
use regex::Regex;
use std::path::Path;
use std::sync::OnceLock;

fn tag_re(cell: &'static OnceLock<Regex>, tag: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(&format!(r"(?s)<dc:{tag}[^>]*>([^<]+)</dc:{tag}>")).unwrap())
}

fn unescape(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&#39;", "'")
        .trim()
        .to_string()
}

pub fn read_opf(path: &Path) -> Option<Ebook> {
    let xml = std::fs::read_to_string(path).ok()?;
    static TITLE: OnceLock<Regex> = OnceLock::new();
    static CREATOR: OnceLock<Regex> = OnceLock::new();
    static LANGUAGE: OnceLock<Regex> = OnceLock::new();

    let title = tag_re(&TITLE, "title").captures(&xml)?.get(1)?.as_str();
    let author = tag_re(&CREATOR, "creator")
        .captures(&xml)
        .and_then(|c| c.get(1))
        .map(|m| unescape(m.as_str()))
        .unwrap_or_default();
    let language = tag_re(&LANGUAGE, "language")
        .captures(&xml)
        .and_then(|c| c.get(1))
        .map(|m| unescape(m.as_str()));

    Some(Ebook {
        title: crate::matcher::preclean_title(&unescape(title)),
        author,
        language,
    })
}
