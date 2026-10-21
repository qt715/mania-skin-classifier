"""mania-skin-classify: classify one osu!mania skin (folder or .osk) and print JSON."""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

from . import __version__
from .classify import classify_skin
from .config import Thresholds
from .sources import open_skin


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(prog="mania-skin-classify", description=__doc__)
    ap.add_argument("skin", type=Path, help="one skin folder (containing skin.ini) or one .osk archive")
    ap.add_argument("-o", "--output", type=Path, help="write JSON here instead of stdout")
    ap.add_argument("--details", action="store_true", help="include per-column evidence and the thresholds used")
    ap.add_argument("--thresholds", type=Path, help="JSON file overriding default thresholds")
    ap.add_argument("--no-content-hash", action="store_true", help="skip hashing every skin file for skin.content_hash")
    ap.add_argument("--compact", action="store_true", help="single-line JSON")
    ap.add_argument("--version", action="version", version=__version__)
    args = ap.parse_args(argv)

    try:
        thresholds = Thresholds.load(args.thresholds)
        source = open_skin(args.skin)
    except (ValueError, OSError) as e:
        print(f"mania-skin-classify: {e}", file=sys.stderr)
        return 2

    result = classify_skin(source, thresholds, details=args.details, content_hash=not args.no_content_hash)
    text = json.dumps(result, ensure_ascii=False, indent=None if args.compact else 2)
    if args.output:
        args.output.write_text(text + "\n", encoding="utf-8")
    else:
        sys.stdout.write(text + "\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
