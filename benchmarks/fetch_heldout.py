#!/usr/bin/env python3
"""Download the held-out benchmark skins (skin.ini + images only) listed in heldout-sources.tsv.

Usage: benchmarks/fetch_heldout.py OUT_DIR
Requires an authenticated `gh` CLI (for the git tree API). Skins are fetched from each repo's
current default branch; labels are keyed by image SHA-256, so any image that changed upstream
simply drops out of the evaluation instead of being scored against a stale label.
"""

from __future__ import annotations

import json
import os
import re
import subprocess
import sys
import urllib.parse
import urllib.request
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

HERE = Path(__file__).resolve().parent


def gh(path: str) -> dict:
    return json.loads(subprocess.run(["gh", "api", path], capture_output=True, check=True, text=True).stdout)


def fetch(out: Path, repo: str, ini_path: str) -> str:
    d = os.path.dirname(ini_path)
    branch = gh(f"repos/{repo}")["default_branch"]
    tree = gh(f"repos/{repo}/git/trees/{branch}?recursive=1")["tree"]
    prefix = f"{d}/" if d else ""
    files = [
        t["path"] for t in tree
        if t["type"] == "blob" and t["path"].startswith(prefix)
        and t["path"][len(prefix):].count("/") <= 2
        and re.search(r"\.(png|jpe?g|ini)$", t["path"], re.I)
    ]
    dest = out / re.sub(r"[^A-Za-z0-9._-]+", "_", f"{repo}__{d or 'root'}")[:120]
    for f in files:
        p = dest / f[len(prefix):]
        p.parent.mkdir(parents=True, exist_ok=True)
        if not p.exists():
            urllib.request.urlretrieve(f"https://raw.githubusercontent.com/{repo}/{branch}/{urllib.parse.quote(f)}", p)
    return f"{dest.name}: {len(files)} files"


def main() -> int:
    out = Path(sys.argv[1])
    out.mkdir(parents=True, exist_ok=True)
    rows = [l.split("\t") for l in (HERE / "heldout-sources.tsv").read_text().splitlines() if l and not l.startswith("#")]
    with ThreadPoolExecutor(8) as ex:
        for msg in ex.map(lambda r: fetch(out, *r), rows):
            print(msg, file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
