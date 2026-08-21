# CLAUDE.md

Guidance for Claude Code when working in this repository.

## What this is

narratarr is an arr-family bridge (soularr's shape): it mirrors an ebook
library into a monitored audiobook collection. Audiobookshelf (or a directory
of Calibre `.opf` sidecars) is the source of "what books do I have";
Listenarr is the target that searches, downloads, and imports; narratarr is
the matchmaker and reconciler in between. Rust, small dependency tree, ships
as a ~27 MB Docker image.

## Commands

```bash
cargo test                    # the whole suite; must stay green
cargo fmt --check             # CI enforces
cargo clippy --all-targets -- -D warnings   # CI enforces; zero warnings
cargo build --release         # ~3 MB binary
docker build -t narratarr:dev .
```

There is no mock-server test rig; API clients are verified against a live
stack. Everything else (matcher, store, config, reconcile plumbing, request
bodies, page parsers) is pure and unit-tested.

## Architecture

One binary, level-based reconciler. Each cycle re-derives desired state from
the sources and converges — no queues, no events, nothing to "catch up" on.

| Module | Role | Purity |
|---|---|---|
| `matcher.rs` | ebook → audiobook candidate scoring and classification | pure, heavily tested |
| `store.rs` | the ONLY owned state: one SQLite table of decisions + retry timers | thin I/O |
| `abs.rs` | Audiobookshelf client (read-only source) | I/O + pure page parser |
| `listenarr.rs` | Listenarr client (check `by-asin`, add monitored) | I/O + pure body builder |
| `audible.rs` | candidate search, Audible public catalog | I/O |
| `opf.rs` | Calibre sidecar fallback source | thin I/O |
| `config.rs` | narratarr.toml (arr idiom: sections per service, `--init`, discovery) | pure |
| `reconcile.rs` | interval parsing, overrides file, review report | pure |
| `main.rs` | flag/config merge, the reconcile loop | glue |

## Invariants — do not break these

1. **Normalize for comparison, never for search queries.** Queries keep
   punctuation (`Abaddon's Gate`); only similarity scoring uses normalized
   forms. This was a real bug; there is a test.
2. **Unknown degrades to `ambiguous`, never to a guess and never to silence.**
   A perfect title with a missing/bogus author is reviewed by a human, not
   auto-added and not silently refused.
3. **Check-then-act on every mutation.** A timed-out add is UNKNOWN, not
   failed; the next cycle's existence check resolves it. Never blind-retry a
   non-idempotent call.
4. **The store holds decisions only.** Every other fact is queried live from
   the system that owns it (ABS owns "what ebooks exist", Listenarr owns
   "what's monitored"). The db must remain deletable and regenerable.
5. **Precision over recall for auto-add.** A wrong match that downloads the
   wrong audiobook is far worse than a correct match parked in review.
6. **Dry-run is the default.** New installs must never mutate anything until
   `apply = true` is an explicit choice.
7. **The daemon survives its dependencies.** A source or target being down
   aborts the cycle, not the process (one-shot mode may still exit nonzero).
8. **Config typos are hard errors** (`deny_unknown_fields`), and the shipped
   `config::EXAMPLE` must always parse — both are pinned by tests.

## The fixture discipline

Every real-world mis-verdict becomes a pinned test in `tests/matcher_tests.rs`
before or alongside the fix. The matcher was validated against a real 236-book
library (157 auto-matched / 14 review / 65 refused, zero known false
positives); the tests encode those verdicts. If a heuristic tweak flips a
pinned verdict, that is a decision to justify, not a test to update casually.

Known matcher limitations (documented, not bugs to "fix" casually): omnibus
ebooks (one ebook → many audiobooks), misspelled ebook metadata, and ebook
language metadata being untrustworthy (that's why language comes from config).

## Releases

CI (`ci.yml`) gates fmt/clippy/test on every push. Pushing a `v*` tag runs
`release.yml`: multi-arch (amd64+arm64) Docker image to GHCR. `Cargo.lock` is
committed on purpose (binary crate).
