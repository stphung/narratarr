//! Request-body contract for the Listenarr add call: camelCase field names
//! matching ASP.NET's default binding, minimal-but-sufficient metadata.

use narratarr::listenarr::{add_request_body, BookMetadata};

#[test]
fn add_body_shape() {
    let m = BookMetadata {
        asin: "B003ZWFO7E".into(),
        title: "The Way of Kings".into(),
        subtitle: None,
        authors: vec!["Brandon Sanderson".into()],
        narrators: vec!["Michael Kramer".into(), "Kate Reading".into()],
        language: Some("english".into()),
    };
    let body = add_request_body(&m, true, false);
    assert_eq!(body["metadata"]["asin"], "B003ZWFO7E");
    assert_eq!(body["metadata"]["authors"][0], "Brandon Sanderson");
    assert_eq!(body["monitored"], true);
    assert_eq!(body["autoSearch"], false);
    // subtitle serializes as null, not omitted — the DTO's fields are nullable
    assert!(body["metadata"]["subtitle"].is_null());
}
