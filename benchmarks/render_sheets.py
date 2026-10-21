#!/usr/bin/env python3
"""Render blind labeling sheets: every distinct note sprite / LN body top of each skin, numbered.

No classifier output is drawn, so labels made from these sheets are independent of it.
Writes OUT/<skin>.notes.png, OUT/<skin>.ln.png and OUT/<skin>.json (index -> sha256).

Usage: benchmarks/render_sheets.py OUT SKIN_DIR [SKIN_DIR...]
"""

from __future__ import annotations

import io
import json
import sys
from pathlib import Path

from PIL import Image, ImageDraw

from mania_skin_classifier.assets import resolve_column
from mania_skin_classifier.skinini import parse_skin_ini
from mania_skin_classifier.sources import open_skin

Image.MAX_IMAGE_PIXELS = None
CELL = 128
LN_ROWS = 240


def checker(w: int, h: int) -> Image.Image:
    im = Image.new("RGBA", (w, h), (90, 90, 90, 255))
    d = ImageDraw.Draw(im)
    for y in range(0, h, 8):
        for x in range(0, w, 8):
            if (x // 8 + y // 8) % 2:
                d.rectangle((x, y, x + 7, y + 7), fill=(130, 130, 130, 255))
    return im


def grid(items: list[tuple[int, Image.Image]], cell_w: int, cell_h: int, cols: int) -> Image.Image:
    rows = max(1, (len(items) + cols - 1) // cols)
    sheet = Image.new("RGBA", (cols * (cell_w + 8) + 8, rows * (cell_h + 26) + 8), (30, 30, 30, 255))
    d = ImageDraw.Draw(sheet)
    for n, (i, im) in enumerate(items):
        x = 8 + (n % cols) * (cell_w + 8)
        y = 8 + (n // cols) * (cell_h + 26)
        bg = checker(cell_w, cell_h)
        im = im.copy()
        im.thumbnail((cell_w, cell_h))
        bg.alpha_composite(im, ((cell_w - im.width) // 2, (cell_h - im.height) // 2))
        sheet.paste(bg, (x, y + 18))
        d.text((x, y), f"#{i}", fill=(255, 255, 0, 255))
    return sheet


def _open(src, path: str, max_rows: int | None = None) -> Image.Image | None:
    """Decoded sprite (optionally only its top rows), or None if it is not a readable image."""
    try:
        im = Image.open(io.BytesIO(src.read(path)))
        if max_rows is not None:
            im = im.crop((0, 0, im.width, min(im.height, max_rows)))
        return im.convert("RGBA")
    except (OSError, ValueError):
        return None


def main() -> int:
    out = Path(sys.argv[1])
    out.mkdir(parents=True, exist_ok=True)
    for arg in sys.argv[2:]:
        if not Path(arg).is_dir() and not arg.lower().endswith(".osk"):
            continue
        src = open_skin(Path(arg))
        if not src.has("skin.ini"):
            continue
        ini = parse_skin_ini(src.read("skin.ini"))
        notes: dict[str, int] = {}
        bodies: dict[str, int] = {}
        note_imgs, body_imgs = [], []
        for keys, sec in sorted(ini.mania.items()):
            for col in range(keys):
                _, a = resolve_column(src, sec.values, keys, col, "")
                if a is not None:
                    h = src.sha256(a.first.path)
                    if h not in notes and (im := _open(src, a.first.path)) is not None:
                        notes[h] = len(notes)
                        note_imgs.append((notes[h], im))
                _, b = resolve_column(src, sec.values, keys, col, "L")
                if b is not None:
                    h = src.sha256(b.first.path)
                    if h not in bodies and (im := _open(src, b.first.path, LN_ROWS)) is not None:
                        bodies[h] = len(bodies)
                        body_imgs.append((bodies[h], im.resize((min(im.width, 96), im.height))))
        if not notes and not bodies:
            continue
        stem = Path(arg).name
        if note_imgs:
            grid(note_imgs, CELL, CELL, 8).save(out / f"{stem}.notes.png")
        if body_imgs:
            grid(body_imgs, 96, LN_ROWS, 10).save(out / f"{stem}.ln.png")
        (out / f"{stem}.json").write_text(json.dumps({
            "name": src.name,
            "notes": {str(i): h for h, i in notes.items()},
            "ln_bodies": {str(i): h for h, i in bodies.items()},
        }, ensure_ascii=False, indent=1))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
