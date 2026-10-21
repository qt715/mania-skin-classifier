# Rust port: bulk classification for skins.osuck.net

The Python package is the reference for behavior. The Rust binary in `rust/` gives the same
answers much faster: one pass over an entire skin library, then the same binary once per new
upload.

## Status

The port is implemented, and its decisions match the Python reference exactly on both
benchmark corpora (`benchmarks/parity.py`):

| Corpus | Skins | Columns | Decision mismatches | IoU / cut-amount drift beyond tolerance |
|---|---|---|---|---|
| Development (maintainer's lazer library) | 43 | 1640 | 0 | 0 |
| Held out (public GitHub skins) | 39 | 1552 | 0 | 0 |

"Decisions" means `key_modes`, `unresolved`, and every per-column note shape and LN cut.
The tolerances are 0.02 IoU and 1 px of cut amount.

Throughput on a 32-thread desktop (NVMe, warm page cache):

| Input | Skins | Wall time | Rate |
|---|---|---|---|
| Skin folders (the two corpora repeated) | 984 | 1.45 s | ~680 skins/s |
| `.osk` archives (development set, 1.9 GB) | 43 | 0.20 s | ~215 skins/s |
| `.osk`, `-j 1` | 43 | 1.6 s | ~37 ms/skin |

Peak RSS was 732 MB with 32 workers, each decoding 40 000 px LN bodies. Use `-j` to bound it.
For comparison, the Python tool takes about 0.3 s per skin as a process and about 34 ms in-process.

## How ck will run it (assumed workflow)

1. **Backfill once.** Every uploaded skin is an `.osk` (or an extracted folder) on disk or a
   bucket mount, with a database row per skin. One command reads them all and writes one
   file to import.
2. **Import.** Load the results into the site database and map them to search tags
   ("4K circles", "7K arrows", "cut LN").
3. **Keep up.** Classify each new upload with the same binary in single-skin mode. The JSON
   schema is the same, so the import code is shared.
4. **Re-run selectively.** `schema_version` and `tool.version` show which rows are stale
   after a rule or threshold change.

## CLI

```sh
# single skin: same JSON as the Python tool
mania-skin-classify path/to/skin.osk [--details] [--no-content-hash] [--compact] [-o out.json]

# bulk
mania-skin-classify scan /srv/osuck/skins -o results.jsonl --csv key_modes.csv -j 16
mania-skin-classify scan --list ids.tsv -o results.jsonl --resume
```

- **`scan DIR`** finds `*.osk` files and folders with a `skin.ini` at their root, recursively.
  It doesn't descend into skin folders, so variant subfolders aren't counted as skins. The `id`
  is the path relative to `DIR`, without `.osk`.
- **`--list FILE`** takes TSV lines `id<TAB>path`, e.g. exported from the database so results
  carry database ids.
- **`-o results.jsonl`** gets one line per skin: `"id"`, then the single-skin object, then
  `"error": null|string`. Each line is flushed as soon as its skin finishes, in completion order.
- **`--csv key_modes.csv`** writes flat rows `id,key_mode,note_shapes,ln_cut,unclassified_columns`,
  with lists joined by `|`. Skins with no mania section have no rows. This is the file for
  `COPY` / `LOAD DATA`.
- **`--resume`** appends, skipping ids that already have an `error: null` line. Failed ids
  are retried and get a new line, so **the last line per id wins** on import.
- **`--content-hash`** is opt-in for `scan`, because it reads every file of every skin. The
  single-skin mode computes it by default, like the Python tool.
- **Errors:** progress goes to stderr. The exit status is 2 only for setup errors (bad
  arguments, unwritable output). A corrupt archive or a panic inside one skin becomes that
  skin's `error` and never stops the run.

## Design

- **No extraction.** The zip central directory is read once. Only `skin.ini` and the images
  it resolves to are inflated.
- **Bounded memory.** PNG LN bodies are streamed row by row (`png::Reader::next_row`),
  keeping per-row maximum alpha plus the first 4096 rows. Notes over 16 MP are subsampled
  while streaming.
- **No OpenCV.** Area/bilinear resize (OpenCV's exact INTER_AREA weights and 11-bit
  bilinear coefficients), connected components, elliptical closing, hole filling and hull
  area are small kernels in `raster.rs`. The binary is pure Rust: static musl Linux,
  Windows (cross-built with `cargo xwin`) and macOS builds need no system libraries.
- **Templates.** Python rasterizes templates at 512 px, rotates and crops them, then
  area-resizes to the sample. Rust renders the same analytic shapes straight at the sample
  size with 4×4 supersampling, keeping pixels at least half covered. That is the same cut as
  area-resize plus `> 127`. Aspect-dependent bar templates emulate OpenCV's anti-aliased edge
  (centres within 0.5 px of the outline). Rendered templates are cached per sample size and
  thread, and IoU is computed on bitsets.
- **Parallelism.** A rayon pool runs one skin per task. A single writer thread receives
  results through a bounded channel. Each skin runs under `catch_unwind`.

## Deviations from the earlier plan

- **No per-skin wall-clock limit.** A Rust thread can't be killed safely. Pathological
  inputs are bounded instead: image dimensions are checked before decoding, large PNGs are
  streamed, and the slowest real skin takes about 220 ms. If a hard limit is ever needed,
  run each skin in a child process.
- **SHA-256 instead of CRC32 dedup.** Within a skin, the analysis cache is keyed by SHA-256 of the
  sprite bytes, which also feeds `--details`. Only referenced sprites are hashed, so the cost
  is small next to decoding.

## Maintaining parity

Any behavior change lands in Python first, then in Rust. Then rerun:

```sh
tools/run_all.sh CORPUS out/py --details --no-content-hash
rust/target/release/mania-skin-classify scan CORPUS -o out/rust.jsonl --details
uv run python benchmarks/parity.py out/rust.jsonl out/py
```

The labels in `benchmarks/` also score the Rust output directly:
`benchmarks/evaluate.py LABELS out/rust.jsonl`.

## Open questions for ck

- Where the library lives: local disk, a bucket mount, or HTTP. That decides whether
  `--list` should accept URLs.
- Which database he uses, and whether the CSV or the JSONL import is easier.
- Whether `unclassified_columns` should hide a mode's tags or show them with a "partial" badge.
