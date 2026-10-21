"""Bounded-memory image access.

Percy-style LN bodies can be tens of thousands of pixels tall. Images above
``STREAM_PIXELS`` are never fully decoded: PNGs are read row by row with pypng, keeping
only a per-row alpha maximum (for LN bodies) or a strided subsample (for note masks).
"""

from __future__ import annotations

import io
from dataclasses import dataclass

import numpy as np
import png
from PIL import Image

# We gate on header dimensions ourselves, so Pillow's bomb check would only add false errors.
Image.MAX_IMAGE_PIXELS = None

STREAM_PIXELS = 16_000_000
MASK_MAX_SIDE = 1024


class ImageError(Exception):
    pass


@dataclass
class Header:
    width: int
    height: int
    format: str
    has_alpha: bool


def header(data: bytes) -> Header:
    try:
        with Image.open(io.BytesIO(data)) as im:
            has_alpha = im.mode in ("RGBA", "LA", "PA") or "transparency" in im.info
            return Header(im.width, im.height, im.format or "?", has_alpha)
    except Exception as e:  # noqa: BLE001 - any decoder failure means unreadable
        raise ImageError(f"unreadable image: {e}") from e


def _png_rows(data: bytes):
    try:
        w, h, rows, _info = png.Reader(bytes=data).asRGBA8()
    except Exception as e:  # noqa: BLE001
        raise ImageError(f"png decode failed: {e}") from e
    return w, h, rows


def body_rows(data: bytes, top_rows: int) -> tuple[np.ndarray, np.ndarray]:
    """Per-row maximum alpha for every row (top first) plus the first ``top_rows`` rows as RGBA.

    Only the top block is ever held at full resolution, so LN bodies tens of thousands of
    pixels tall cost O(width * top_rows) memory.
    """
    hd = header(data)
    if hd.width * hd.height <= STREAM_PIXELS or hd.format != "PNG":
        rgba = _decode_rgba(data, hd, draft=False)
        return rgba[:, :, 3].max(axis=1), rgba[:top_rows].copy()
    _w, h, rows = _png_rows(data)
    out = np.empty(h, dtype=np.uint8)
    top = []
    try:
        for i, row in enumerate(rows):
            px = np.frombuffer(row, dtype=np.uint8)
            out[i] = px[3::4].max()
            if i < top_rows:
                top.append(px.reshape(-1, 4).copy())
    except Exception as e:  # noqa: BLE001
        raise ImageError(f"png decode failed: {e}") from e
    return out, np.stack(top)


def _decode_rgba(data: bytes, hd: Header, draft: bool) -> np.ndarray:
    try:
        with Image.open(io.BytesIO(data)) as im:
            if draft and hd.format == "JPEG" and max(hd.width, hd.height) > MASK_MAX_SIDE:
                im.draft("RGB", (MASK_MAX_SIDE, MASK_MAX_SIDE))
            # Animated formats: the first frame is what Pillow exposes by default.
            return np.asarray(im.convert("RGBA"))
    except Exception as e:  # noqa: BLE001
        raise ImageError(f"decode failed: {e}") from e


def load_rgba(data: bytes) -> tuple[np.ndarray, int]:
    """RGBA array plus the integer subsample stride applied (1 = full resolution)."""
    hd = header(data)
    if hd.width * hd.height <= STREAM_PIXELS or hd.format != "PNG":
        rgba = _decode_rgba(data, hd, draft=True)
        stride = max(1, round(hd.width / rgba.shape[1])) if rgba.shape[1] else 1
        return rgba, stride
    stride = int(np.ceil(max(hd.width, hd.height) / MASK_MAX_SIDE))
    _w, _h, rows = _png_rows(data)
    kept = []
    try:
        for i, row in enumerate(rows):
            if i % stride == 0:
                kept.append(np.frombuffer(row, dtype=np.uint8).reshape(-1, 4)[::stride].copy())
    except Exception as e:  # noqa: BLE001
        raise ImageError(f"png decode failed: {e}") from e
    return np.stack(kept), stride
