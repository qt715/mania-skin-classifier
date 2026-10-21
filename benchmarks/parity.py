#!/usr/bin/env python3
"""Compare Rust `scan --details` JSONL against Python `--details` results (the reference).

Usage: benchmarks/parity.py RUST.jsonl PY_RESULT_DIR
PY_RESULT_DIR holds `<id>.json` files from `tools/run_all.sh DIR OUT --details`.
Exit status 1 if any decision differs (key modes, unresolved list, per-column shape or cut).
Feature drift (IoU, cut amount) is reported but does not fail unless it exceeds tolerances.
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

IOU_TOL = 0.02
BLANK_TOL = 1.0


def main() -> int:
    rust = {}
    for line in Path(sys.argv[1]).read_text().splitlines():
        r = json.loads(line)
        rust[r["id"]] = r
    py_dir = Path(sys.argv[2])
    fails, drift, compared, columns = [], [], 0, 0
    for rid, r in sorted(rust.items()):
        pf = py_dir / f"{rid}.json"
        if not pf.exists():
            fails.append(f"{rid}: no Python result")
            continue
        p = json.loads(pf.read_text())
        compared += 1
        if r["error"] is not None:
            fails.append(f"{rid}: rust error {r['error']}")
            continue
        if r["key_modes"] != p["key_modes"]:
            for km in sorted(set(r["key_modes"]) | set(p["key_modes"]), key=int):
                if r["key_modes"].get(km) != p["key_modes"].get(km):
                    fails.append(f"{rid} {km}K: rust {r['key_modes'].get(km)} != py {p['key_modes'].get(km)}")
        if r["unresolved"] != p["unresolved"]:
            fails.append(f"{rid}: unresolved differs ({len(r['unresolved'])} vs {len(p['unresolved'])})")
        for km, pcols in p.get("details", {}).items():
            for pc, rc in zip(pcols, r["details"].get(km, [])):
                columns += 1
                pn, rn = pc["note"], rc["note"]
                where = f"{rid} {km}K col {pc['column']}"
                if pn.get("shape") != rn.get("shape"):
                    fails.append(f"{where}: shape rust {rn.get('shape')} ({rn.get('reason')}) != py {pn.get('shape')} ({pn.get('reason')})")
                for cls, v in pn.get("features", {}).get("iou", {}).items():
                    rv = rn.get("features", {}).get("iou", {}).get(cls)
                    if rv is None or abs(rv - v) > IOU_TOL:
                        drift.append(f"{where}: iou[{cls}] rust {rv} py {v}")
                pl, rl = pc["ln"], rc["ln"]
                if pl["cut"] != rl["cut"]:
                    fails.append(f"{where}: cut rust {rl['cut']} ({rl.get('reason')}) != py {pl['cut']} ({pl.get('reason')})")
                pb, rb = pl.get("body_top_blank_px"), rl.get("body_top_blank_px")
                if (pb is None) != (rb is None) or (pb is not None and abs(pb - rb) > BLANK_TOL):
                    drift.append(f"{where}: top_blank_px rust {rb} py {pb}")
    print(f"skins compared: {compared}, columns: {columns}, decision mismatches: {len(fails)}, feature drift beyond tolerance: {len(drift)}")
    for f in fails[:50]:
        print("FAIL", f)
    for d in drift[:30]:
        print("DRIFT", d)
    return 1 if fails else 0


if __name__ == "__main__":
    raise SystemExit(main())
