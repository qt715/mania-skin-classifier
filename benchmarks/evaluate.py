#!/usr/bin/env python3
"""Score classifier output (run with --details) against image-hash-keyed labels.

Labels are keyed by SHA-256 of the image file, so they apply to any copy of the skin
(folder, .osk, lazer import) and stay valid when skins are renamed.

Usage: benchmarks/evaluate.py LABELS.json RESULT.json [RESULT.json...]
Reports: per-sprite confusion matrix (distinct images), per-column accuracy, per-key-mode
exact match of note_shapes and ln_cut (labels aggregated with the classifier's own rules).
"""

from __future__ import annotations

import collections
import json
import sys
from pathlib import Path

SHAPES = ("circle", "arrow", "bar", "etc")


def agg_cut(cols: list[str]) -> str:
    if "yes" in cols and "no" in cols:
        return "mixed"
    if "unknown" in cols:
        return "unknown"
    return "all" if "yes" in cols else "none"


def main() -> int:
    labels = json.loads(Path(sys.argv[1]).read_text())
    note_labels: dict[str, str] = labels["notes"]
    body_labels: dict[str, str] = labels["ln_bodies"]

    sprite_pairs: dict[str, tuple[str, str]] = {}
    body_pairs: dict[str, tuple[str, str]] = {}
    col_note = collections.Counter()
    col_ln = collections.Counter()
    km_shapes = collections.Counter()
    km_cut = collections.Counter()
    mismatches: list[str] = []
    unlabeled = 0

    for f in sys.argv[2:]:
        r = json.loads(Path(f).read_text())
        if "details" not in r:
            raise SystemExit(f"{f}: run the classifier with --details")
        skin = r["skin"]["name"]
        for km, cols in r["details"].items():
            truth_shapes, truth_cuts, complete = set(), [], True
            for c in cols:
                note = c["note"]
                sha = note.get("sha256")
                if sha is None:  # unresolved: nothing to label
                    complete = False
                elif sha in note_labels:
                    truth = note_labels[sha]
                    sprite_pairs[sha] = (truth, note["shape"])
                    col_note[truth == note["shape"]] += 1
                    if truth in SHAPES:
                        truth_shapes.add(truth)
                    else:
                        complete = False
                else:
                    unlabeled += 1
                    complete = False
                ln = c["ln"]
                bsha = ln.get("body_sha256")
                if bsha is None:
                    truth_cuts.append("unknown")
                elif bsha in body_labels:
                    truth = "yes" if body_labels[bsha] == "cut" and ln["body_style"] == 1 else "no"
                    body_pairs[bsha] = (body_labels[bsha], ln["cut"])
                    col_ln[truth == ln["cut"]] += 1
                    truth_cuts.append(truth)
                else:
                    unlabeled += 1
                    truth_cuts.append("unlabeled")
            pred = r["key_modes"][km]
            if complete and not pred["unclassified_columns"]:
                ok = sorted(truth_shapes) == sorted(pred["note_shapes"])
                km_shapes[ok] += 1
                if not ok:
                    mismatches.append(f"{skin} {km}K shapes: truth {sorted(truth_shapes)} pred {pred['note_shapes']}")
            if "unlabeled" not in truth_cuts:
                t = agg_cut(truth_cuts)
                ok = t == pred["ln_cut"]
                km_cut[ok] += 1
                if not ok:
                    mismatches.append(f"{skin} {km}K ln_cut: truth {t} pred {pred['ln_cut']}")

    classes = ("circle", "arrow", "bar", "etc", "unknown")
    print(f"distinct note sprites: {len(sprite_pairs)} (unlabeled column refs: {unlabeled})")
    print("confusion (rows = label, cols = prediction):")
    print(f"{'':>9}" + "".join(f"{c:>9}" for c in classes))
    conf = collections.Counter(sprite_pairs.values())
    for t in classes:
        print(f"{t:>9}" + "".join(f"{conf[(t, p)]:>9}" for p in classes))
    correct = sum(v for (t, p), v in conf.items() if t == p)
    print(f"sprite accuracy: {correct}/{len(sprite_pairs)} = {correct / max(1, len(sprite_pairs)):.3f}")
    print(f"column note accuracy: {col_note[True]}/{sum(col_note.values())} = {col_note[True] / max(1, sum(col_note.values())):.3f}")
    bc = collections.Counter(("yes" if t == "cut" else "no", p) for t, p in body_pairs.values())
    print(f"distinct LN bodies: {len(body_pairs)}; (label, pred) counts: {dict(bc)}")
    print(f"column LN accuracy: {col_ln[True]}/{sum(col_ln.values())} = {col_ln[True] / max(1, sum(col_ln.values())):.3f}")
    print(f"key-mode note_shapes exact match: {km_shapes[True]}/{sum(km_shapes.values())}")
    print(f"key-mode ln_cut match: {km_cut[True]}/{sum(km_cut.values())}")
    if mismatches:
        print("mismatches:")
        for m in mismatches:
            print("  " + m)
    wrong = [(sha, t, p) for sha, (t, p) in sprite_pairs.items() if t != p]
    if wrong:
        print("wrong sprites (sha256, label, prediction):")
        for sha, t, p in wrong:
            print(f"  {sha} {t} -> {p}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
