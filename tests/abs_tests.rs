//! Page-parser contract for the Audiobookshelf items API.

use narratarr::abs::parse_items_page;
use serde_json::json;

#[test]
fn parses_items_and_applies_preclean() {
    let page = json!({
        "total": 236,
        "results": [
            {"id": "a", "media": {"metadata": {
                "title": "Cline, Ernest - Armada: A Novel",
                "authorName": "Ernest Cline", "language": null}}},
            {"id": "b", "media": {"metadata": {
                "title": "  ", "authorName": "Nobody"}}},          // blank title -> dropped
            {"id": "c", "media": {"metadata": {
                "title": "Anathem", "authorName": "Neal Stephenson", "language": "en"}}}
        ]
    });
    let (books, total) = parse_items_page(&page);
    assert_eq!(total, 236);
    assert_eq!(books.len(), 2);
    // the same preclean as the OPF path, so state keys are identical across modes
    assert_eq!(books[0].title, "Armada: A Novel");
    assert_eq!(books[1].language.as_deref(), Some("en"));
}
