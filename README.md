# shelfspoken

**Your shelf, spoken.** shelfspoken mirrors an ebook library into a monitored
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

## Try it

```
cargo run --release -- /path/to/calibre/library --limit 25 [--verbose] [--lang en]
```

Reads Calibre `.opf` sidecars, searches Audible's public catalog, and prints a
verdict per book: `OK` (auto-match), `??` (review), `--` (refused).

## Roadmap

- [x] Matching engine + regression corpus
- [ ] SQLite decision store with retry backoff
- [ ] Audiobookshelf (source) and Listenarr (target) API clients
- [ ] The reconcile loop, with check-then-act idempotency
- [ ] Docker image + compose snippet

`reference-python/` holds the original Python prototype the Rust
implementation was ported from; it will be removed once the Rust version
surpasses it.
