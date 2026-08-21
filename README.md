# narratarr

**The narrator for your bookshelf.**

narratarr watches the ebooks you already own and quietly builds the audiobook
collection to match — no requests, no lists to maintain, no input at all. Add
an ebook to your library; some time later, the audiobook is just *there*.

It's a bridge in the [*arr ecosystem](https://github.com/Ravencentric/awesome-arr),
in the same spirit as [soularr](https://github.com/mrusse/soularr): it doesn't
search indexers or download anything itself. It sits between two apps that
already do their jobs well:

- **[Audiobookshelf](https://github.com/advplyr/audiobookshelf)** — your book
  server, and narratarr's *source of truth* for "what books do I have"
- **[Listenarr](https://github.com/Listenarrs/Listenarr)** — the audiobook
  manager (Sonarr-for-audiobooks) that searches your indexers, grabs releases
  through your download clients, and imports the files

```
ebooks in Audiobookshelf ──▶ narratarr ──▶ Listenarr ──▶ indexers + download client
        ▲                   (matching)    (monitored)             │
        └────────────────── audiobooks imported back ◀────────────┘
```

## How it works

Every cycle, narratarr lists your ebook library, matches each book against
Audible's catalog, and sorts the results into three lanes:

- **Matched** — high-confidence hits (title *and* author confirmed) are added
  to Listenarr as monitored, automatically.
- **Review** — plausible-but-unconfirmed candidates (franchise-prefixed
  titles, missing author metadata, suspiciously similar titles by the same
  author) go into a markdown **review report** with paste-ready accept/reject
  lines. You answer in a plain-text **overrides file**; narratarr acts on your
  answers next cycle.
- **Refused** — books with no audiobook edition, derivative junk ("Summary
  of…"), dramatized adaptations, and garbage metadata are skipped, and
  not-found books are automatically retried a month later.

The matcher is deliberately paranoid — **precision over recall** — because a
wrong auto-match downloads the wrong audiobook. Against a real 236-book
library it auto-matched 157, parked 14 for review, refused 65, and made zero
wrong matches. It survives real-world metadata filth: reversed author names,
`Author - Title` filename junk, `(2010)` year tags, semicolon author lists,
abridged editions, and ebooks whose language metadata simply lies.

narratarr is **stateless by design** except for one small SQLite file of match
decisions — delete it and everything regenerates. It never writes to
Audiobookshelf, and every action against Listenarr is check-then-act, so
re-runs are always safe.

## Setup

You'll need Audiobookshelf and Listenarr already running (Listenarr wired to
your indexers and download client). Then:

**1. Add narratarr to your compose file:**

```yaml
  narratarr:
    image: ghcr.io/stphung/narratarr:latest
    container_name: narratarr
    volumes:
      - ./narratarr/config:/config
    restart: unless-stopped
```

**2. Start it once.** The first run writes a commented `narratarr.toml` into
the config volume and waits for you.

**3. Edit `narratarr/config/narratarr.toml`:**

```toml
[general]
interval = "6h"          # reconcile cadence; comment out to run once
language = "en"
apply = false            # dry-run until you're ready

[audiobookshelf]
url = "http://audiobookshelf:80"     # compose service names, not host names
token = "..."            # ABS: user icon -> Settings -> Users -> API token
library = "Books"        # the EBOOK library to mirror

[listenarr]
url = "http://listenarr:4545"
api_key = "..."          # Listenarr: Settings -> General -> API key
```

**4. Restart and read the logs.** In dry-run, narratarr prints a verdict per
book (`OK` auto-match, `??` review, `--` refused) and what it *would* send to
Listenarr, and writes `review.md` to the config volume:

```
OK  Project Hail Mary        | Andy Weir     -> Project Hail Mary [B08G9PRS1K] total=1.000
??  The Last Guardian        | Jeff Grubb    -> Warcraft: The Last Guardian [1945683260]
--  The Algorithm Design Manual | Steven Skiena
```

**5. Flip `apply = true`** when the output looks right. Matched books flow
into Listenarr as monitored; Listenarr takes it from there; finished
audiobooks land back in Audiobookshelf.

**6. Answer the review report** (optional, whenever you like): copy lines
from `review.md` into `overrides.txt`:

```
jeff grubb|the last guardian = 1945683260     # yes, that's the right book
bart farkas|starcraft prima s official strategy guide = skip
```

That's the whole workflow: logs you can read, one file to answer, nothing
else to operate.

## Without Audiobookshelf

narratarr can also read a directory of Calibre-style `.opf` sidecars directly
— set `books_dir = "/books"` in `[general]` (and mount it into the container),
or run the binary against a path: `narratarr /path/to/library`.

## Configuration reference

| Setting | Default | Meaning |
|---|---|---|
| `general.interval` | *(unset)* | Reconcile cadence (`90s`, `30m`, `6h`, `1d`). Unset = one cycle, then exit. |
| `general.language` | `"en"` | Preferred audiobook language. Trusted over ebook metadata, which lies. |
| `general.apply` | `false` | Dry-run until true. |
| `general.auto_search` | `false` | Ask Listenarr to search immediately on add. |
| `general.limit` | `0` | Max books per cycle; `0` = all. |
| `general.state_file` | *(unset)* | The decisions database. Strongly recommended: `/config/narratarr.db`. |
| `general.report_file` | *(unset)* | Review report path, rewritten each cycle. |
| `general.overrides_file` | *(unset)* | Your accept/reject answers. |
| `general.books_dir` | *(unset)* | Calibre-directory source (used when `[audiobookshelf]` is absent). |
| `audiobookshelf.*` | — | `url`, `token`, `library`. |
| `listenarr.*` | — | `url`, `api_key`. |

Every setting has a CLI-flag override (`--interval`, `--apply`, `--limit`,
`--state`, `--report`, `--overrides`, `--abs`, `--abs-token`, `--abs-library`,
`--listenarr`, `--listenarr-key`, `--lang`, `--verbose`); flags win over the
config file. `narratarr --init` writes a starter config to the current
directory.

## Good to know

- **Nothing happens without `apply = true`** — a fresh install can never
  surprise you.
- Unknown config keys are startup errors, not silent no-ops — typos can't
  quietly disable a setting.
- If Audiobookshelf or Listenarr is down, the cycle is skipped and retried at
  the next interval; the daemon doesn't crash-loop.
- Not-found books are retried monthly — audiobook availability changes, and
  your indexers' rate limits are respected (one catalog query per second).
- Known limitations: omnibus ebooks (one file containing a whole series)
  aren't expanded into per-book audiobooks yet, and badly misspelled ebook
  metadata can defeat search ("Farenheit 451" is real and undefeated).

## Development

```
cargo test      # matcher fixtures, store contracts, config contracts
cargo clippy --all-targets -- -D warnings
```

The matcher's verdicts against a real library are pinned as tests: if you
tweak a heuristic and a fixture flips, that's a design decision to discuss,
not a test to update. Found a bad match in the wild? A failing fixture in
`tests/matcher_tests.rs` is the perfect bug report — and the perfect PR.

## License

MIT. Not affiliated with Audible or Amazon; narratarr queries Audible's
public catalog API for metadata only.
