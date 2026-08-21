"""shelfspoken.opf — read title/author/language from Calibre .opf sidecar files."""

from __future__ import annotations

import xml.etree.ElementTree as ET
from pathlib import Path

_NS = {
    "dc": "http://purl.org/dc/elements/1.1/",
    "opf": "http://www.idpf.org/2007/opf",
}


def read_opf(path: Path) -> dict | None:
    try:
        root = ET.parse(path).getroot()
    except ET.ParseError:
        return None
    md = root.find("opf:metadata", _NS)
    if md is None:
        return None

    def first(tag: str) -> str | None:
        el = md.find(f"dc:{tag}", _NS)
        return el.text.strip() if el is not None and el.text else None

    title = first("title")
    if not title:
        return None
    return {
        "title": title,
        "author": first("creator") or "",
        "language": first("language"),
    }
