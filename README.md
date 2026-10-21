# mania-skin-classifier

Classifies one osu!mania skin per run: for every key mode the skin declares, which note
shapes it uses (`circle`, `arrow`, `bar`, `etc`) and whether its long notes are cut
("Percy" LNs). Output is JSON meant to be imported into a skin database such as osuck.

There are two implementations with identical decisions. The Python package is the reference.
The Rust binary in `rust/` runs bulk passes over thousands of skins at hundreds of skins per
second; see [docs/rust-port.md](docs/rust-port.md).

## Rust binary (bulk runs)

Download a prebuilt binary from the GitHub releases, or build with `cargo build --release`
in `rust/`.

```sh
mania-skin-classify path/to/skin.osk                       # one skin, same JSON as Python
mania-skin-classify scan /srv/skins -o results.jsonl --csv key_modes.csv
mania-skin-classify scan --list ids.tsv -o results.jsonl --resume   # id<TAB>path lines
```

`results.jsonl` holds one record per skin (`id`, the single-skin object, `error`).
`key_modes.csv` holds one row per skin and key mode, ready for a database import.

## Python reference: install and run

```sh
uv sync
uv run mania-skin-classify "path/to/Skin Folder"       # or path/to/skin.osk
uv run mania-skin-classify skin.osk --details -o out.json
```

| Option | Effect |
|---|---|
| `--details` | add per-column evidence (files, hashes, features, thresholds) |
| `--thresholds FILE` | JSON object overriding any field of `Thresholds` in `src/mania_skin_classifier/config.py` |
| `--no-content-hash` | skip `skin.content_hash` (hashes every file in the skin) |
| `--compact` | single-line JSON |

Exit code 2 means the input is not a skin folder or `.osk`, or the thresholds file is invalid.
Problems inside a skin (bad images, missing files) never fail the run; they show up as
`unresolved` entries, `unknown` values and `warnings`.

## Output

```json
{
  "schema_version": 1,
  "tool": {"name": "mania-skin-classifier", "version": "0.1.0"},
  "skin": {"input": "...", "kind": "folder", "name": "From [General] Name", "content_hash": "…", "ini_version": "2.5"},
  "key_modes": {
    "4": {"note_shapes": ["circle"], "ln_cut": "all", "unclassified_columns": []},
    "7": {"note_shapes": ["arrow", "bar"], "ln_cut": "mixed", "unclassified_columns": [3]}
  },
  "unresolved": [{"key_mode": 7, "column": 3, "role": "note", "lookup": "mania-noteS", "configured": false}],
  "warnings": []
}
```

- `key_modes`: exactly the valid `[Mania]` sections (`Keys:` 1–10, 12, 14, 16, 18). Modes the
  skin doesn't declare are never inferred, even if osu! could render them from fallback assets.
- `note_shapes`: the set of shapes over the mode's columns, in the fixed order circle, arrow, bar, etc.
- `unclassified_columns`: columns whose note sprite is missing (`unresolved`) or couldn't be
  classified (`unknown`). Their shapes are absent from `note_shapes`.
- `ln_cut`: `all` / `none` when every column agrees, `mixed` when cut and uncut columns
  both exist, otherwise `unknown` (some column's LN body is missing or unreadable).
- `skin.content_hash`: SHA-256 over sorted `(lowercased path, file SHA-256)` pairs. The same
  skin as a folder or as an `.osk` hashes the same.

## How it decides

**Parsing** mirrors osu!lazer's `LegacyManiaSkinDecoder`: field and section names are
case-sensitive, `[Mania]` repeats, lines before `Keys:` belong to that section, and when a key
count repeats, the first section wins.

**Sprite resolution** mirrors lazer's legacy skin lookups: `NoteImage#` (0-based), otherwise
`mania-note1` / `mania-note2` / `mania-noteS` by distance from the stage edge, with `S` for the middle
column in odd key counts. `@2x` files are preferred (scale 2), `name-0…` animations take
precedence (frame 0 is analysed), and paths are case-insensitive with `\` treated as `/`. The
tail falls back from `#T` to the head (`#H`), then the note. A name with no matching file is
`unresolved`; it does not fall back to a default sprite.

**Note shape**: the sprite's opaque silhouette is extracted (alpha ≥ 45% of its peak alpha,
or the border colour is removed for alpha-less images). Specks are dropped, the silhouette is
closed and hole-filled, trimmed, then compared by IoU against analytic templates fitted to its
bounding box at 8 rotations: ellipse (circle); stemmed, DDR, chevron, triangle and dart arrows;
rectangle, rounded, stadium, chamfered and pointed bars; diamond, square, hexagon, octagon,
plus, star and heart (etc).
- Aspect gates: circles need a long:short side ratio of at most 1.35, bars at least 1.3, and
  squares less than 1.3.
- Arrows are never point-symmetric. A silhouette whose 180° self-IoU is at least 0.93 can't be
  an arrow. A near-tie between a point-symmetric etc template and an arrow goes to the arrow
  when the silhouette isn't point-symmetric.
- Best IoU below 0.8, or a largest part holding under 60% of the area, gives `etc`. A tie
  within 0.02 between circle, arrow and bar gives `unknown`.

**Cut (Percy) LN** per column: the body is `NoteImage#L` (or its default). The style is
`NoteBodyStyle#`, then `NoteBodyStyle`, then 1. A column is cut when all of these hold:
- the style is 1;
- the @2x-normalized body height is at least 1000 px;
- the cut amount *d* (background rows above where the body starts, in logical px; the
  same *d* that Percy Skin Editor edits) is at least 4.

Background means transparent rows, a faint "ghost" of the LN end (alpha below 25% of the
body's alpha), or an opaque uniform cover band that differs from the body. Decorations shorter
than 32 px above the body are skipped. A transparent tail raises the confidence from 0.75 to 0.95.
Bodies are streamed row by row with pypng when larger than 16 MP, so 40 000 px bodies use
O(width × 4096) memory.

## Benchmark

Labels in `benchmarks/labels.json` are keyed by image SHA-256. Note labels were made blind
from `benchmarks/render_sheets.py` sheets, which never show classifier output. Reproduce:

```sh
tools/run_all.sh SKINS_DIR out/results --details --no-content-hash
uv run python benchmarks/evaluate.py benchmarks/labels.json out/results/*.json
```

Results are in [docs/benchmark.md](docs/benchmark.md).

## osu!lazer libraries

lazer stores skins as hashed blobs indexed by `client.realm`. `tools/lazer_export.py OUT`
copies the realm file (it never opens the live database), reads it with realm-js, and writes
each imported skin as `OUT/<skin-id>/` symlinks plus `OUT/index.json`:

```sh
(cd tools/lazer-realm-dump && REALM_DISABLE_ANALYTICS=1 npm ci)
uv run python tools/lazer_export.py /tmp/lazer-skins
uv run tools/run_all.sh /tmp/lazer-skins out/results
```

## Known limits

- `SpecialStyle` is not applied to the fallback layout for even key counts. lazer ignores it
  for texture fallback, and stable's exact layout isn't public.
- lazer's default `NoteBodyStyle` (stretch below skin version 2.5, otherwise repeat) is not used.
  The default is 1, as in the skin.ini specification.
- Only frame 0 of an animated note is classified.
