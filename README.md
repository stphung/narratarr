# narratarr

**The narrator for your bookshelf.** narratarr mirrors an ebook library into a monitored
audiobook collection: it reads the books you already own, finds each one's
audiobook edition, and (eventually) hands the wanted list to an audiobook
automation stack — so your library builds its own audio twin with no input
from you.

Status: **early development.** The matching engine — the make-or-break piece —
is built and validated against a real 236-book Calibre library:

- **157 auto-matched** with exact Audible ASINs and zero known false positives
- **14 parked for human review** (franchise-prefixed titles, brand-named
  editions, suspiciously-adjacent titles by the same author)
- **65 correctly refused** (no audiobook exists, or the ebook metadata is
  garbage)

## Design

A level-based reconciler, not an event pipeline: each cycle recomputes desired
state from the ebook library, diffs it against the audiobook manager, and
converges. One SQLite table of match *decisions* is the only owned state —
every other fact is queried live from the system that owns it.

The matcher is a pure function (`src/matcher.rs`), hardened against
real-world library filth: reversed `Last, First` authors, semicolon author
lists, credentials and site-spam in author fields, author names embedded in
title fields, `(2010)` year tags, `_`-for-`:` filename substitutions, lying
language metadata, "Summary of…" derivative ebooks, dramatized adaptations,
abridged editions, and edition-suffix noise ("15th Anniversary Edition").
Every lesson is pinned in `tests/matcher_tests.rs`.

Two rules learned the hard way:

1. **Normalize for comparison, never for queries** — search engines want
   `Abaddon's Gate`, not `abaddon s gate`.
2. **Bad metadata degrades to human review, never to a guess** — a perfect
   title with an unconfirmable author is `ambiguous`, not `matched`.

## Quick start

```
narratarr --init          # writes a commented narratarr.toml
$EDITOR narratarr.toml    # point it at Audiobookshelf and Listenarr
narratarr                 # runs a dry-run cycle and writes the review report
```

narratarr starts in **dry-run** and only reports what it would do; flip
`apply = true` once the output looks right. Config is discovered at
`/config/narratarr.toml` (Docker) or `./narratarr.toml`; every setting can
also be overridden by a CLI flag. Verdicts per book: `OK` (auto-match),
`??` (review report), `--` (refused).

Accept or reject review-lane books by copying the paste-ready lines from the
report into your overrides file:

```
brandon sanderson|mistborn: secret history = B01DPMS8JC   # accept this ASIN
bart farkas|starcraft prima s official strategy guide = skip
```

Without Audiobookshelf, point it at a directory of Calibre `.opf` sidecars
instead: `narratarr /path/to/library`.

## Roadmap

- [x] Matching engine + regression corpus
- [x] SQLite decision store with retry backoff
- [x] Audiobookshelf (source) and Listenarr (target) API clients
- [x] The reconcile loop: `--interval`, review report, human overrides file
- [x] narratarr.toml config (arr-style: `--init`, /config discovery, dry-run default)
- [ ] Structured logging + graceful per-cycle error handling
- [ ] Docker image + compose snippet

`reference-python/` holds the original Python prototype the Rust
implementation was ported from; it will be removed once the Rust version
surpasses it.
