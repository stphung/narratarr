"""shelfspoken.match — pure matching logic.

Given ebook metadata and a list of audiobook candidates, score each candidate
and classify the outcome. No I/O, no state: everything here is unit-testable
with plain dicts.

An Ebook is a dict:      {"title": str, "author": str, "language": str|None}
A Candidate is a dict:   {"asin": str, "title": str, "subtitle": str|None,
                          "authors": [str], "narrators": [str],
                          "format_type": str|None,   # "unabridged"/"abridged"
                          "language": str|None, "runtime_min": int|None}
"""

from __future__ import annotations

import re
from difflib import SequenceMatcher

# ── thresholds ──────────────────────────────────────────────────────────
MATCH_THRESHOLD = 0.85      # >= this: safe to auto-add
AMBIGUOUS_THRESHOLD = 0.60  # >= this: park for human review
MIN_AUTHOR_SCORE = 0.60     # a great title with the wrong author is not a match

# Candidate titles matching these are derivative junk, not the book.
# Audible is full of them and they score deceptively well on title similarity.
_JUNK_PATTERNS = re.compile(
    r"""\b(
        summary\s+(?:of|and\s+analysis)|
        workbook\s+for|
        study\s+guide|
        key\s+takeaways|
        analysis\s+of|
        conversation\s+starters|
        trivia|
        in\s+\d+\s+minutes
    )\b""",
    re.IGNORECASE | re.VERBOSE,
)

_NOISE_IN_AUTHOR = re.compile(
    r"""(
        \[.*?\]|                # "[Skiena, Steven S.]" duplicated bracket junk
        &\s*\S*\.(?:com|net|org)\S*|   # "& chenjin5.com" site spam
        \b(?:phd|md|esq|jr|sr|iii?)\b\.?
    )""",
    re.IGNORECASE | re.VERBOSE,
)

_ARTICLE_SUFFIX = re.compile(r",\s*(the|a|an)$", re.IGNORECASE)  # "Alloy of Law, The"
_YEAR_SUFFIX = re.compile(r"\s*\(\d{4}\)\s*$")                   # "... (2010)"
_SERIES_HINT = re.compile(  # "A Mistborn Novel", "Book 3 of ...", "(Stormlight Archive #1)"
    r"""(
        \ba\s+.{2,40}?\s+novel\b|
        \bbook\s+\d+\b|
        \#\d+
    )""",
    re.IGNORECASE | re.VERBOSE,
)


def normalize_title(raw: str) -> str:
    """Lowercased title with articles, years, punctuation and filename artifacts removed."""
    t = raw.strip()
    t = _YEAR_SUFFIX.sub("", t)
    t = _ARTICLE_SUFFIX.sub("", t)          # "Alloy of Law, The" -> "Alloy of Law"
    t = t.replace("_", ":")                  # filename-safe colon back to a colon
    t = re.sub(r"[^\w\s:]", " ", t.lower())
    t = re.sub(r"\s+", " ", t).strip()
    return t


def base_title(normalized: str) -> str:
    """The part before any subtitle: 'alloy of law: a mistborn novel' -> 'alloy of law'."""
    head = normalized.split(":", 1)[0].strip()
    head = _SERIES_HINT.sub("", head).strip()
    return head or normalized


def query_title(raw: str) -> str:
    """Light cleanup for use as a SEARCH QUERY: fix filename artifacts but keep
    punctuation — search engines want "Abaddon's Gate", not "abaddon s gate"."""
    t = raw.strip()
    t = _YEAR_SUFFIX.sub("", t)
    t = _ARTICLE_SUFFIX.sub("", t)          # "Alloy of Law, The" -> "Alloy of Law"
    t = t.replace("_", ":")
    t = t.split(":", 1)[0].strip()          # drop subtitle: editions disagree on them
    t = re.sub(r"\s*\([^)]*\)\s*$", "", t)  # "(wow-4)"-style trailing series tags
    return t or raw.strip()


def normalize_author(raw: str) -> str:
    """'Bryson, Bill' -> 'bill bryson'; strips credentials and bracket/site junk."""
    a = _NOISE_IN_AUTHOR.sub(" ", raw)
    a = a.strip().strip("&").strip()
    if "," in a:  # "Last, First" -> "First Last" (only if it looks like that shape)
        parts = [p.strip() for p in a.split(",") if p.strip()]
        if len(parts) == 2 and " " not in parts[0]:
            a = f"{parts[1]} {parts[0]}"
    a = re.sub(r"[^\w\s]", " ", a.lower())
    a = re.sub(r"\s+", " ", a).strip()
    return a


def _ratio(a: str, b: str) -> float:
    if not a or not b:
        return 0.0
    return SequenceMatcher(None, a, b).ratio()


def title_score(ebook_title: str, cand_title: str, cand_subtitle: str | None) -> float:
    """Similarity that tolerates subtitle asymmetry between editions."""
    e_full = normalize_title(ebook_title)
    c_full = normalize_title(
        cand_title if not cand_subtitle else f"{cand_title}: {cand_subtitle}"
    )
    e_base, c_base = base_title(e_full), base_title(normalize_title(cand_title))

    candidates = [
        _ratio(e_full, c_full),
        _ratio(e_base, c_base),
        # one side carries a subtitle the other lacks
        _ratio(e_base, c_full),
        _ratio(e_full, c_base),
    ]
    score = max(candidates)
    # exact base-title equality is a strong signal even when subtitles diverge
    if e_base and e_base == c_base:
        score = max(score, 0.95)
    return score


def author_score(ebook_author: str, cand_authors: list[str]) -> float:
    """Best similarity against any credited author; last-name agreement is weighted."""
    e = normalize_author(ebook_author)
    if not e or not cand_authors:
        return 0.0
    best = 0.0
    e_last = e.split()[-1]
    for raw in cand_authors:
        c = normalize_author(raw)
        if not c:
            continue
        s = _ratio(e, c)
        if c.split()[-1] == e_last:      # same surname rescues initials/diacritics cases
            s = max(s, 0.75 + 0.25 * s)
        best = max(best, s)
    return best


def score_candidate(ebook: dict, cand: dict) -> dict:
    """Score one candidate. Returns component scores and a total in [0, 1]."""
    t = title_score(ebook["title"], cand.get("title", ""), cand.get("subtitle"))
    a = author_score(ebook.get("author", ""), cand.get("authors", []))

    total = 0.45 * t + 0.45 * a
    penalties, bonuses = [], []

    full_cand_title = f"{cand.get('title','')} {cand.get('subtitle') or ''}"
    if _JUNK_PATTERNS.search(full_cand_title):
        total -= 0.60
        penalties.append("derivative-work")
    fmt = (cand.get("format_type") or "").lower()
    if fmt == "abridged":
        total -= 0.15
        penalties.append("abridged")
    elif fmt == "unabridged":
        total += 0.05
        bonuses.append("unabridged")
    e_lang = (ebook.get("language") or "").lower()[:2]
    c_lang = (cand.get("language") or "").lower()[:2]
    if e_lang and c_lang:
        if e_lang == c_lang:
            total += 0.05
            bonuses.append("language")
        else:
            total -= 0.30
            penalties.append("language-mismatch")

    return {
        "asin": cand.get("asin"),
        "title": cand.get("title"),
        "narrators": cand.get("narrators", []),
        "title_score": round(t, 3),
        "author_score": round(a, 3),
        "total": round(max(0.0, min(1.0, total)), 3),
        "penalties": penalties,
        "bonuses": bonuses,
    }


def match(ebook: dict, candidates: list[dict]) -> dict:
    """Classify an ebook against its candidate list.

    Returns {"status": "matched"|"ambiguous"|"not_found",
             "best": scored|None, "scored": [scored, ...]}
    """
    scored = sorted(
        (score_candidate(ebook, c) for c in candidates),
        key=lambda s: s["total"],
        reverse=True,
    )
    viable = [s for s in scored if "derivative-work" not in s["penalties"]]
    best = viable[0] if viable else None

    if best is None:
        status = "not_found"
    elif best["total"] >= MATCH_THRESHOLD and best["author_score"] >= MIN_AUTHOR_SCORE:
        status = "matched"
    elif best["total"] >= AMBIGUOUS_THRESHOLD:
        status = "ambiguous"
    else:
        status = "not_found"

    return {"status": status, "best": best, "scored": scored}
