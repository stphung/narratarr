//! Listenarr client — the target side of the bridge.
//!
//! Two calls only: "is this ASIN in the library?" and "add it, monitored."
//! Every mutation is check-then-act (a timed-out add is UNKNOWN, not failed,
//! and gets re-verified next cycle), and Listenarr's own ASIN dedup on add is
//! the server-side backstop.

use serde_json::{json, Value};
use std::time::Duration;

pub struct Client {
    base_url: String,
    api_key: String,
}

#[derive(Debug, Clone)]
pub struct BookMetadata {
    pub asin: String,
    pub title: String,
    pub subtitle: Option<String>,
    pub authors: Vec<String>,
    pub narrators: Vec<String>,
    pub language: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum AddOutcome {
    Added,
    AlreadyExists,
}

/// Pure request-body construction, kept separate so it is testable.
pub fn add_request_body(m: &BookMetadata, monitored: bool, auto_search: bool) -> Value {
    json!({
        "metadata": {
            "asin": m.asin,
            "title": m.title,
            "subtitle": m.subtitle,
            "authors": m.authors,
            "narrators": m.narrators,
            "language": m.language,
            "source": "Audible",
        },
        "monitored": monitored,
        "autoSearch": auto_search,
    })
}

impl Client {
    pub fn new(base_url: &str, api_key: &str) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key: api_key.to_string(),
        }
    }

    /// Is an audiobook with this ASIN already in Listenarr's library?
    pub fn exists_by_asin(&self, asin: &str) -> Result<bool, Box<dyn std::error::Error>> {
        let url = format!("{}/api/v1/library/by-asin/{asin}", self.base_url);
        match ureq::get(&url)
            .set("X-Api-Key", &self.api_key)
            .timeout(Duration::from_secs(15))
            .call()
        {
            Ok(_) => Ok(true),
            Err(ureq::Error::Status(404, _)) => Ok(false),
            Err(e) => Err(Box::new(e)),
        }
    }

    /// Add a book to the library. Relies on Listenarr's own ASIN dedup as a
    /// second line of defense behind the caller's exists check.
    pub fn add(
        &self,
        m: &BookMetadata,
        monitored: bool,
        auto_search: bool,
    ) -> Result<AddOutcome, Box<dyn std::error::Error>> {
        let url = format!("{}/api/v1/library/add", self.base_url);
        let resp = ureq::post(&url)
            .set("X-Api-Key", &self.api_key)
            .set("Content-Type", "application/json")
            .timeout(Duration::from_secs(60))
            .send_string(&add_request_body(m, monitored, auto_search).to_string())?;
        let body: Value = resp.into_json().unwrap_or(Value::Null);
        if body["alreadyExists"].as_bool().unwrap_or(false) {
            Ok(AddOutcome::AlreadyExists)
        } else {
            Ok(AddOutcome::Added)
        }
    }
}
