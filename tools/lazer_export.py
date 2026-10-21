#!/usr/bin/env python3
"""Materialize every user skin imported into osu!lazer as a plain skin folder of symlinks.

lazer stores skins as SHA-256-named blobs indexed by client.realm, so the classifier (which
takes one skin folder) cannot read them directly. This copies client.realm (never opening
the live database), dumps the skin -> file mapping with tools/lazer-realm-dump, and writes
``<out>/<skin-id>/<original filename>`` symlinks into lazer's ``files/`` store.

Usage: tools/lazer_export.py [--data-dir ~/.local/share/osu] OUT_DIR
Requires: node, and ``npm ci`` in tools/lazer-realm-dump (realm-js native addon).
"""

from __future__ import annotations

import argparse
import json
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("out", type=Path)
    ap.add_argument("--data-dir", type=Path, default=Path.home() / ".local/share/osu")
    args = ap.parse_args()

    files_dir = args.data_dir / "files"
    with tempfile.TemporaryDirectory() as tmp:
        copy = Path(tmp) / "client.realm"
        shutil.copyfile(args.data_dir / "client.realm", copy)
        dump = subprocess.run(["node", str(HERE / "lazer-realm-dump" / "dump.mjs"), str(copy)], check=True, capture_output=True, timeout=300)
    skins = json.loads(dump.stdout)["skins"]

    args.out.mkdir(parents=True, exist_ok=True)
    index = []
    for s in skins:
        if s["protected"] or s["delete_pending"] or not s["files"]:
            continue  # built-in lazer skins have no files; pending deletions are not "installed"
        dest = args.out / s["id"]
        if dest.exists():
            shutil.rmtree(dest)
        missing = 0
        for f in s["files"]:
            blob = files_dir / f["hash"][0] / f["hash"][:2] / f["hash"]
            if not blob.is_file():
                missing += 1
                continue
            rel = Path(f["filename"].replace("\\", "/"))
            if rel.is_absolute() or ".." in rel.parts:
                continue
            target = dest / rel
            target.parent.mkdir(parents=True, exist_ok=True)
            target.symlink_to(blob)
        index.append({"id": s["id"], "name": s["name"], "creator": s["creator"], "lazer_hash": s["hash"], "folder": str(dest), "missing_files": missing})
    (args.out / "index.json").write_text(json.dumps(index, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(f"exported {len(index)} skins to {args.out}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
