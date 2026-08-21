//! The review web UI: one screen, the ambiguous queue as cards.
//!
//! The web thread only ever writes DECISIONS to the store; the reconcile loop
//! remains the sole component that talks to Listenarr, and picks up accepted
//! books on its next cycle. LAN-facing and unauthenticated by design — do not
//! expose it to the internet.

use crate::matcher::{match_ebook, query_title, Ebook};
use crate::store::{now_epoch, Decision, DecisionStatus, Store};
use crate::{audible, matcher};
use serde_json::{json, Value};
use std::path::PathBuf;

const PAGE: &str = include_str!("web/index.html");

pub fn serve(port: u16, state_path: PathBuf) {
    let server = match tiny_http::Server::http(("0.0.0.0", port)) {
        Ok(s) => {
            println!("review UI listening on :{port}");
            s
        }
        Err(e) => {
            eprintln!("!! review UI failed to bind :{port}: {e}");
            return;
        }
    };
    for mut request in server.incoming_requests() {
        let store = match Store::open(&state_path) {
            Ok(s) => s,
            Err(e) => {
                let _ = respond_json(request, 500, &json!({"error": e.to_string()}));
                continue;
            }
        };
        let url = request.url().to_string();
        let method = request.method().clone();
        let (path, query) = match url.split_once('?') {
            Some((p, q)) => (p.to_string(), q.to_string()),
            None => (url.clone(), String::new()),
        };
        match (method, path.as_str()) {
            (tiny_http::Method::Get, "/") => {
                let header = tiny_http::Header::from_bytes(
                    &b"Content-Type"[..],
                    &b"text/html; charset=utf-8"[..],
                )
                .unwrap();
                let _ = request.respond(tiny_http::Response::from_string(PAGE).with_header(header));
            }
            (tiny_http::Method::Get, "/api/queue") => {
                let _ = match queue_json(&store) {
                    Ok(v) => respond_json(request, 200, &v),
                    Err(e) => respond_json(request, 500, &json!({"error": e})),
                };
            }
            (tiny_http::Method::Get, "/api/alternatives") => {
                let key = query_param(&query, "key");
                let _ = match key.map(|k| alternatives_json(&store, &k)) {
                    Some(Ok(v)) => respond_json(request, 200, &v),
                    Some(Err(e)) => respond_json(request, 500, &json!({"error": e})),
                    None => respond_json(request, 400, &json!({"error": "missing key"})),
                };
            }
            (tiny_http::Method::Post, "/api/decide") => {
                let mut body = String::new();
                let _ = request.as_reader().read_to_string(&mut body);
                let _ = match decide(&store, &body) {
                    Ok(v) => respond_json(request, 200, &v),
                    Err(e) => respond_json(request, 400, &json!({"error": e})),
                };
            }
            _ => {
                let _ = respond_json(request, 404, &json!({"error": "not found"}));
            }
        }
    }
}

fn respond_json(request: tiny_http::Request, status: u16, v: &Value) -> Result<(), std::io::Error> {
    let header =
        tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap();
    request.respond(
        tiny_http::Response::from_string(v.to_string())
            .with_status_code(status)
            .with_header(header),
    )
}

fn query_param(query: &str, name: &str) -> Option<String> {
    query.split('&').find_map(|kv| {
        let (k, v) = kv.split_once('=')?;
        if k == name {
            Some(urldecode(v))
        } else {
            None
        }
    })
}

fn urldecode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => match u8::from_str_radix(&s[i + 1..i + 3], 16) {
                Ok(v) => {
                    out.push(v);
                    i += 3;
                }
                Err(_) => {
                    out.push(b'%');
                    i += 1;
                }
            },
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The ambiguous queue as JSON for the page.
pub fn queue_json(store: &Store) -> Result<Value, String> {
    let all = store.all().map_err(|e| e.to_string())?;
    let items: Vec<Value> = all
        .iter()
        .filter(|d| d.status == DecisionStatus::Ambiguous)
        .map(|d| {
            json!({
                "key": d.ebook_key,
                "ebook_title": d.ebook_title,
                "ebook_author": d.ebook_author,
                "candidate": d.note,
                "asin": d.asin,
                "confidence": d.confidence,
            })
        })
        .collect();
    Ok(json!({"items": items}))
}

/// Live search for a book's top candidates, scored, with cover art.
fn alternatives_json(store: &Store, key: &str) -> Result<Value, String> {
    let d = store
        .get(key)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "unknown key".to_string())?;
    // Prefer the stored raw ebook identity; fall back to the normalized key.
    let title = d
        .ebook_title
        .clone()
        .unwrap_or_else(|| key.split('|').nth(1).unwrap_or(key).to_string());
    let author = d
        .ebook_author
        .clone()
        .unwrap_or_else(|| key.split('|').next().unwrap_or("").to_string());
    let q = query_title(&title);
    let primary = matcher::primary_author(&author);
    let author_opt = if primary.is_empty() {
        None
    } else {
        Some(primary.as_str())
    };
    let mut cands = audible::search(&q, author_opt).map_err(|e| e.to_string())?;
    if cands.is_empty() && author_opt.is_some() {
        cands = audible::search(&q, None).map_err(|e| e.to_string())?;
    }
    let ebook = Ebook {
        title: title.clone(),
        author,
        language: None,
    };
    let result = match_ebook(&ebook, &cands);
    let items: Vec<Value> = result
        .scored
        .iter()
        .take(6)
        .filter_map(|s| {
            let asin = s.asin.clone()?;
            let cand = cands.iter().find(|c| c.asin.as_deref() == Some(&asin))?;
            Some(json!({
                "asin": asin,
                "title": cand.title,
                "subtitle": cand.subtitle,
                "authors": cand.authors,
                "narrators": cand.narrators,
                "runtime_min": cand.runtime_min,
                "image_url": cand.image_url,
                "total": s.total,
                "penalties": s.penalties,
            }))
        })
        .collect();
    Ok(json!({"key": key, "ebook_title": title, "items": items}))
}

/// Apply a human verdict from the page. Body: {"key","action":"accept"|"skip","asin"?}
pub fn decide(store: &Store, body: &str) -> Result<Value, String> {
    let v: Value = serde_json::from_str(body).map_err(|e| e.to_string())?;
    let key = v["key"].as_str().ok_or("missing key")?.to_string();
    let action = v["action"].as_str().ok_or("missing action")?;
    let prior = store.get(&key).map_err(|e| e.to_string())?;
    let attempts = prior.as_ref().map(|d| d.attempts).unwrap_or(0);
    let (status, asin, note) = match action {
        "skip" => (DecisionStatus::Skipped, None, "web review: rejected"),
        "accept" => {
            let asin = v["asin"]
                .as_str()
                .map(str::to_string)
                .or_else(|| prior.as_ref().and_then(|d| d.asin.clone()))
                .ok_or("accept needs an asin")?;
            (DecisionStatus::Matched, Some(asin), "web review: accepted")
        }
        _ => return Err("action must be accept or skip".into()),
    };
    let d = Decision {
        ebook_key: key.clone(),
        status,
        asin,
        confidence: Some(1.0),
        attempts,
        next_retry: None,
        updated_at: now_epoch(),
        note: Some(note.into()),
        ebook_title: prior.as_ref().and_then(|d| d.ebook_title.clone()),
        ebook_author: prior.as_ref().and_then(|d| d.ebook_author.clone()),
    };
    store.record(&d).map_err(|e| e.to_string())?;
    Ok(json!({"ok": true, "key": key, "status": d.status.as_str()}))
}
