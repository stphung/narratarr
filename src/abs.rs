//! Audiobookshelf client — the source side of the bridge.
//!
//! Read-only: lists the ebooks in one ABS library. ABS is the owner of "what
//! books do I have"; narratarr never writes to it. (Completed audiobooks reach
//! ABS through Listenarr's imports, not through us.)

use crate::matcher::{preclean_title, Ebook};
use serde_json::Value;
use std::time::Duration;

pub struct Client {
    base_url: String,
    token: String,
}

/// (id, name, media_type) of an ABS library.
pub type LibraryInfo = (String, String, String);

const PAGE_SIZE: usize = 100;

impl Client {
    pub fn new(base_url: &str, token: &str) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            token: token.to_string(),
        }
    }

    fn get(&self, path: &str) -> Result<Value, Box<dyn std::error::Error>> {
        Ok(ureq::get(&format!("{}{path}", self.base_url))
            .set("Authorization", &format!("Bearer {}", self.token))
            .timeout(Duration::from_secs(30))
            .call()?
            .into_json()?)
    }

    /// (id, name, media_type) for every library on the server.
    pub fn libraries(&self) -> Result<Vec<LibraryInfo>, Box<dyn std::error::Error>> {
        let v = self.get("/api/libraries")?;
        Ok(v["libraries"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .map(|l| {
                        (
                            l["id"].as_str().unwrap_or_default().to_string(),
                            l["name"].as_str().unwrap_or_default().to_string(),
                            l["mediaType"].as_str().unwrap_or_default().to_string(),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default())
    }

    /// All items of a library as matcher Ebooks, fully paginated.
    pub fn ebooks(&self, library_id: &str) -> Result<Vec<Ebook>, Box<dyn std::error::Error>> {
        let mut out = Vec::new();
        let mut page = 0usize;
        loop {
            let v = self.get(&format!(
                "/api/libraries/{library_id}/items?limit={PAGE_SIZE}&page={page}"
            ))?;
            let (mut books, total) = parse_items_page(&v);
            out.append(&mut books);
            page += 1;
            if out.len() >= total || page > 1000 {
                break;
            }
        }
        Ok(out)
    }
}

/// Pure page parser, testable with canned JSON. Returns (books, server_total).
pub fn parse_items_page(v: &Value) -> (Vec<Ebook>, usize) {
    let total = v["total"].as_u64().unwrap_or(0) as usize;
    let books = v["results"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|item| {
                    let md = &item["media"]["metadata"];
                    let title = md["title"].as_str()?.trim();
                    if title.is_empty() {
                        return None;
                    }
                    Some(Ebook {
                        // same preclean as the OPF path, so both source modes
                        // produce identical titles and identical state keys
                        title: preclean_title(title),
                        author: md["authorName"].as_str().unwrap_or_default().to_string(),
                        language: md["language"].as_str().map(str::to_string),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    (books, total)
}
