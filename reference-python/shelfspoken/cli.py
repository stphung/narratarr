"""Try shelfspoken's matcher against a directory of Calibre-style ebooks.

    python -m shelfspoken.cli /path/to/books --limit 10
"""

from __future__ import annotations

import argparse
import sys
import time
from pathlib import Path

from . import audible
from .match import match, query_title
from .opf import read_opf


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("books_dir", type=Path)
    ap.add_argument("--limit", type=int, default=10)
    ap.add_argument("--verbose", action="store_true", help="show runner-up candidates")
    args = ap.parse_args()

    opfs = sorted(args.books_dir.glob("*.opf"))[: args.limit]
    if not opfs:
        print(f"no .opf files found in {args.books_dir}", file=sys.stderr)
        return 1

    counts = {"matched": 0, "ambiguous": 0, "not_found": 0, "error": 0}
    for opf_path in opfs:
        ebook = read_opf(opf_path)
        if not ebook:
            counts["error"] += 1
            print(f"?? unreadable opf: {opf_path.name}")
            continue

        try:
            candidates = audible.search(query_title(ebook["title"]), ebook.get("author") or None)
            if not candidates and ebook.get("author"):
                # bad author metadata is common; retry on title alone and let
                # the scorer decide (a wrong author can then never auto-match)
                candidates = audible.search(query_title(ebook["title"]))
        except Exception as exc:  # noqa: BLE001 - report and continue the sweep
            counts["error"] += 1
            print(f"!! search failed for {ebook['title']!r}: {exc}")
            continue

        result = match(ebook, candidates)
        counts[result["status"]] += 1
        icon = {"matched": "OK", "ambiguous": "??", "not_found": "--"}[result["status"]]
        best = result["best"]
        line = f"{icon}  {ebook['title'][:48]:<48} | {ebook['author'][:22]:<22}"
        if best:
            line += (
                f" -> {best['title'][:40]:<40} [{best['asin']}]"
                f" t={best['title_score']} a={best['author_score']} total={best['total']}"
            )
            if best["penalties"]:
                line += f" ({','.join(best['penalties'])})"
        print(line)
        if args.verbose:
            for s in result["scored"][1:4]:
                print(f"      runner-up: {s['title'][:52]:<52} total={s['total']}")
        time.sleep(1)  # be polite to the catalog API

    print(f"\n{counts['matched']} matched / {counts['ambiguous']} ambiguous / "
          f"{counts['not_found']} not found / {counts['error']} errors")
    return 0


if __name__ == "__main__":
    sys.exit(main())
