# Benchmark (2026-10-21)

The classifier was evaluated on two labeled sets. Labels are keyed by image SHA-256.

| Set | Skins | Distinct note sprites | Distinct LN bodies | Role |
|---|---|---|---|---|
| `labels.json`: maintainer's osu!lazer library | 41 with `[Mania]` | 162 | 86 | development (thresholds tuned here) |
| `labels-heldout.json`: public GitHub skins (`heldout-sources.tsv`) | 42 fetched, 16 duplicates removed | 96 | 81 | held out (thresholds frozen before labeling) |

Note sprites were labeled blind: independent labelers saw only `render_sheets.py` sheets
(numbered sprites on a checkerboard, no classifier output). LN bodies were labeled by visually
checking the top of every body that is at least 1000 logical px tall. All of them showed
background above the start of the body, i.e. the Percy structure. Shorter bodies are tiling
textures.

## Results

| Metric | Development | Held out |
|---|---|---|
| Note sprite accuracy (distinct images) | 161/162 (99.4%) | 93/96 (96.9%) |
| Note accuracy per column | 1541/1543 (99.9%) | 1225/1340 (91.4%) |
| LN cut accuracy per column | 1448/1448 (100%) | 1332/1332 (100%) |
| Key mode `note_shapes` exact match | 253/255 | 202/216 |
| Key mode `ln_cut` match | 274/274 | 242/242 |

Held-out confusion matrix (rows are labels, columns are predictions):

| | circle | arrow | bar | etc |
|---|---|---|---|---|
| circle | 16 | 0 | 0 | 0 |
| arrow | 0 | 12 | 0 | 0 |
| bar | 0 | 0 | 62 | 3 |
| etc | 0 | 0 | 0 | 3 |

## Errors

- **Held out, osu!doodle.** Three hand-drawn, wobbly bars. The best bar IoU is 0.787, just
  under the 0.8 template threshold, so the result is `etc`. This one skin accounts for every
  held-out column error (14 key modes × its columns). The failure is conservative:
  `etc`, never a wrong shape class.
- **Development, sigma "-debug" variant.** A circle note with a debug filename label
  stuck to its rim is classified as a hexagon (`etc`).
- **Coverage.** No held-out sprite was predicted `unknown`. No circle, arrow or bar was ever
  confused with another of those three.

## Tuning history (development set only)

1. **First pass:** blank-top *fraction* for LN cuts. It failed on every Percy skin: real bodies
   are 20 000–40 000 px tall with 20–160 px of background, i.e. 0.1–0.5%. Replaced with the
   absolute cut amount *d* (Percy Skin Editor's definition). Faint "ghost" ends and opaque
   black cover bands count as background, and labels above the body are skipped.
2. **Shapes:** added DDR-style arrow templates, the point-symmetry rule (arrow vs. diamond)
   and speck removal, and lowered the bar aspect gate from 1.5 to 1.3. Development sprite
   accuracy went from 82.1% to 99.4%.

## Limits of this evidence

- **Cut LN data is easy.** All 45 tall bodies are cut and all 122 short bodies are not.
  Neither set has a tall body with no background on top, a stretched (`NoteBodyStyle: 0`)
  Percy body, or a lazer-adjusted body with *d* = 0. Unit tests cover these synthetically.
- **Shape mix is narrow.** Only 3 held-out sprites (8 overall) are `etc`, and they cover few kinds
  of shapes.

## Runtime

Python, run in-process on the development library: median 34 ms per skin, slowest 222 ms.
Running one process per skin, all 43 skins take 13.6 s; process start-up (OpenCV import)
dominates. Peak RSS for a single skin is at most 264 MB, with 40 000 px LN bodies.

Rust (v0.2.0) scored against the same labels gives identical numbers on both sets. Its
decisions match Python on all 3192 columns (`benchmarks/parity.py`). It classifies 984 skin
folders in 1.45 s on 32 threads, and the 43 development skins as `.osk` in 0.20 s.
Details are in [rust-port.md](rust-port.md).

## Reproduce

```sh
uv run python tools/lazer_export.py /tmp/lazer-skins          # development set (needs the library)
uv run python benchmarks/fetch_heldout.py /tmp/heldout         # held-out set
tools/run_all.sh /tmp/heldout out/heldout --details --no-content-hash
uv run python benchmarks/evaluate.py benchmarks/labels-heldout.json out/heldout/*.json
rust/target/release/mania-skin-classify scan /tmp/heldout -o out/heldout.jsonl --details
uv run python benchmarks/evaluate.py benchmarks/labels-heldout.json out/heldout.jsonl
uv run python benchmarks/parity.py out/heldout.jsonl out/heldout
```
