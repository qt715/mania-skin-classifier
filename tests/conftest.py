from __future__ import annotations

from pathlib import Path

import cv2
import numpy as np
import pytest
from PIL import Image


def rgba(w: int, h: int) -> np.ndarray:
    return np.zeros((h, w, 4), np.uint8)


def draw(mask: np.ndarray, colour=(200, 120, 255)) -> np.ndarray:
    img = np.zeros((*mask.shape, 4), np.uint8)
    img[mask > 0, :3] = colour
    img[:, :, 3] = mask
    return img


def circle(size=128) -> np.ndarray:
    m = np.zeros((size, size), np.uint8)
    cv2.circle(m, (size // 2, size // 2), size // 2 - 6, 255, -1, cv2.LINE_AA)
    return draw(m)


def ring(size=128) -> np.ndarray:
    m = np.zeros((size, size), np.uint8)
    cv2.circle(m, (size // 2, size // 2), size // 2 - 6, 255, 12, cv2.LINE_AA)
    return draw(m)


def poly(points, w=128, h=128) -> np.ndarray:
    m = np.zeros((h, w), np.uint8)
    cv2.fillPoly(m, [np.array(points, np.int32)], 255, cv2.LINE_AA)
    return draw(m)


def stem_arrow_left() -> np.ndarray:
    return poly([(8, 64), (64, 8), (64, 40), (120, 40), (120, 88), (64, 88), (64, 120)])


def chevron_up() -> np.ndarray:
    return poly([(64, 10), (120, 66), (120, 110), (64, 54), (8, 110), (8, 66)])


def rounded_bar() -> np.ndarray:
    m = np.zeros((48, 160), np.uint8)
    cv2.rectangle(m, (20, 4), (140, 43), 255, -1)
    cv2.circle(m, (20, 24), 20, 255, -1)
    cv2.circle(m, (140, 24), 20, 255, -1)
    return draw(m)


def diamond() -> np.ndarray:
    return poly([(64, 6), (122, 64), (64, 122), (6, 64)])


def square() -> np.ndarray:
    return poly([(10, 10), (118, 10), (118, 118), (10, 118)])


def percy_body(width=64, height=4000, blank=50) -> np.ndarray:
    img = rgba(width, height)
    img[blank:, 8:width - 8] = (180, 180, 180, 255)
    return img


def save(path: Path, arr: np.ndarray) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    Image.fromarray(arr, "RGBA").save(path)


@pytest.fixture
def make_skin(tmp_path):
    def _make(ini: str, files: dict[str, np.ndarray]) -> Path:
        root = tmp_path / "skin"
        root.mkdir(exist_ok=True)
        (root / "skin.ini").write_text(ini, encoding="utf-8")
        for name, arr in files.items():
            save(root / name, arr)
        return root

    return _make
