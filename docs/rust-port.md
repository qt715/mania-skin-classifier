# Rust port: bulk classification for skins.osuck.net

The Python tool is the reference for behavior. The Rust port exists for throughput: one
pass over an entire skin library (thousands to tens of thousands of `.osk` files) that returns
everything osuck needs to tag mania skins, followed by the same binary run once per new upload.

## How ck will run it (assumed workflow)

1. **Backfill once.** He has every uploaded skin as an `.osk` (or an extracted folder) on disk
   or in a bucket mount, plus a database row per skin. He wants a single command that reads them
   all and writes one machine-readable file to import.
2. **Import.** He loads the results into the site database and maps them to search tags
   ("4K circles", "7K arrows", "cut LN").
3. **Keep up.** He classifies each new upload with the same binary in single-skin mode, using the
   same JSON schema, so the import code is shared.
4. **Re-run selectively.** When a rule or threshold improves, `schema_version` and
   `tool.version` show which rows are stale. He re-runs only those rows, or everything if it's cheap.

So the port has to be one static binary with no OpenCV, Python, or Node. It has to be
restartable after a crash or Ctrl-C, keep one bad skin from stopping the run, and produce output
that loads into a database without custom parsing.

## CLI contract

```sh
# single skin (identical output to the Python tool)
mania-skin-classify path/to/skin.osk

# bulk
mania-skin-classify scan /srv/osuck/skins -o results.jsonl --csv key_modes.csv -j 16
mania-skin-classify scan --list ids.tsv -o results.jsonl --resume
```

- `scan DIR`: recursively finds `*.osk`, plus folders containing `skin.ini`. The `id` is
  the path relative to `DIR` without its extension, so `12345.osk` gets id `12345`.
- `--list FILE`: TSV `id<TAB>path` exported from the database, so results key on database ids
  instead of file names.
- `-o results.jsonl`: one line per skin. Each line is the single-skin JSON object plus `"id"`
  and `"error": null|string`. Lines are flushed as soon as each skin finishes.
- `--csv key_modes.csv`: flat rows `id,key_mode,note_shapes,ln_cut,unclassified_columns`.
  Shapes are joined with `|`. This is the file to load with `COPY`/`LOAD DATA`; skins with no mania
  section have no rows.
- `--resume`: reads the existing JSONL and skips ids already present with `error == null`.
- `-j N`: worker threads (default: all cores). `--details` is the same as in the Python tool.
- Progress, rate and ETA go to stderr. Exit status is non-zero only for setup errors, never for a
  bad skin.

## Throughput design

- **No extraction.** Read the zip central directory, then inflate only `skin.ini` and the
  images it resolves to. A typical skin needs 2–12 of its several hundred files.
- **Cheap dedup.** Within a skin, sprites are shared across columns and key modes. Key the
  analysis cache by the zip entry's CRC32 + size, then confirm with SHA-256 only when emitting
  hashes.
- **Bounded memory.** Check PNG/JPEG headers before decoding. Note sprites decode once and are
  downscaled to a 160 px silhouette. LN bodies stream with `png::Reader::next_row`, keeping
  the per-row maximum alpha plus the first 4096 RGBA rows. That handles the 40 000 px Percy
  bodies common in real skins in about 1–2 MB.
- **Parallelism.** Spread skins across a rayon pool, one skin per task and no nested
  parallelism, and send results through a bounded channel to a single writer thread. Wrap each
  skin in `catch_unwind` and give it a wall-clock limit, so a panic or a pathological image
  becomes that skin's `error`.
- **Dependencies.** `zip`, `png`, `jpeg-decoder` (or `zune-jpeg`), `rayon`, `serde_json`,
  `sha2`, `clap`. Morphology, connected components and hole filling are small hand-written
  kernels; hole filling is a flood fill from the border. Templates are rasterized once at
  startup.

## Porting rules

- **Parity before speed.** Run the Python tool with `--details` over the benchmark corpus
  to produce golden files. The Rust output must match `key_modes`, `unresolved` and every
  per-column `shape` / `cut` exactly, with IoU features within 0.01. CI fails on any
  difference.
- **Ported as-is:** the skin.ini line rules, sprite resolution order, templates (analytic
  polygons; see `shapes.py`), aspect and symmetry gates, and the cut-amount analysis (`ln.py`).
- **Thresholds stay external.** The `Thresholds` JSON format is shared, so tuning never
  needs a rebuild.
- **Root resolution.** An `.osk` whose `skin.ini` sits in a single subfolder is re-rooted
  there. Nested variant folders inside a skin are ignored, because osu! loads only the root
  `skin.ini`.

## Open questions for ck

- Where the library lives (local disk, bucket mount, or HTTP), which decides whether
  `--list` should accept URLs.
- The target database, and whether CSV or JSONL import is easier for him.
- Whether he wants `unclassified_columns` to hide a mode's tags, or to show them with a
  "partial" badge.
