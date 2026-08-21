//! Candidate retrieval from Audible's public catalog API (unauthenticated,
//! read-only). The only network I/O in the matching path.

use crate::matcher::Candidate;
use serde_json::Value;

const API: &str = "https://api.audible.com/1.0/catalog/products";
const GROUPS: &str = "contributors,product_attrs,product_desc,series";

pub fn search(title: &str, author: Option<&str>) -> Result<Vec<Candidate>, Box<dyn std::error::Error>> {
    let mut req = ureq::get(API)
        .query("title", title)
        .query("num_results", "20")
        .query("response_groups", GROUPS)
        .query("products_sort_by", "Relevance")
        .set("User-Agent", "narratarr/0.1");
    if let Some(a) = author {
        req = req.query("author", a);
    }
    let payload: Value = req.timeout(std::time::Duration::from_secs(20)).call()?.into_json()?;

    let products = payload["products"].as_array().cloned().unwrap_or_default();
    Ok(products.iter().map(to_candidate).collect())
}

fn names(v: &Value) -> Vec<String> {
    v.as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|p| p["name"].as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

fn to_candidate(p: &Value) -> Candidate {
    Candidate {
        asin: p["asin"].as_str().map(str::to_string),
        title: p["title"].as_str().unwrap_or_default().to_string(),
        subtitle: p["subtitle"].as_str().map(str::to_string),
        authors: names(&p["authors"]),
        narrators: names(&p["narrators"]),
        format_type: p["format_type"].as_str().map(str::to_string),
        language: p["language"].as_str().map(str::to_string),
        runtime_min: p["runtime_length_min"].as_i64(),
    }
}
