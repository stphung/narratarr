"""shelfspoken.audible — candidate retrieval from Audible's public catalog API.

Unauthenticated, read-only. Returns plain Candidate dicts (see match.py).
This is the only network I/O in the matching path, kept separate so match()
stays pure.
"""

from __future__ import annotations

import json
import urllib.parse
import urllib.request

_API = "https://api.audible.com/1.0/catalog/products"
_GROUPS = "contributors,product_attrs,product_desc,series"


def search(title: str, author: str | None = None, num_results: int = 20, timeout: int = 20) -> list[dict]:
    params = {
        "title": title,
        "num_results": str(num_results),
        "response_groups": _GROUPS,
        "products_sort_by": "Relevance",
    }
    if author:
        params["author"] = author
    url = f"{_API}?{urllib.parse.urlencode(params)}"
    req = urllib.request.Request(url, headers={"User-Agent": "shelfspoken/0.1"})
    with urllib.request.urlopen(req, timeout=timeout) as resp:
        payload = json.load(resp)
    return [_to_candidate(p) for p in payload.get("products", [])]


def _to_candidate(p: dict) -> dict:
    return {
        "asin": p.get("asin"),
        "title": p.get("title") or "",
        "subtitle": p.get("subtitle"),
        "authors": [a.get("name", "") for a in p.get("authors") or []],
        "narrators": [n.get("name", "") for n in p.get("narrators") or []],
        "format_type": p.get("format_type"),
        "language": p.get("language"),
        "runtime_min": p.get("runtime_length_min"),
    }
