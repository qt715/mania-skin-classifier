"""Note sprite shape classification: circle / arrow / bar / etc, with unknown kept separate.

The opaque silhouette is trimmed, hole-filled, and compared against analytic templates
fitted to its bounding box at 8 rotations (IoU). Bounding-box fitting makes matching
independent of sprite resolution and padding; rotation covers per-column arrow directions.
Aspect gates stop a stretched circle template from claiming wide ovals and a stretched
rectangle from claiming squares.
"""

from __future__ import annotations

import functools
import math
from dataclasses import dataclass

import cv2
import numpy as np

from .config import Thresholds

SHAPES = ("circle", "arrow", "bar", "etc")
WORK = 160  # longest side of the normalized silhouette
TEMPLATE_RES = 512
# etc templates that are unchanged by a 180-degree turn; arrows never are.
POINT_SYMMETRIC = frozenset({"diamond", "square", "round_square", "hexagon_flat", "hexagon_pointy", "octagon", "plus", "star4"})


@dataclass
class ShapeResult:
    shape: str  # circle | arrow | bar | etc | unknown
    confidence: float
    reason: str
    features: dict


def _poly(points, res=TEMPLATE_RES) -> np.ndarray:
    img = np.zeros((res, res), np.uint8)
    pts = np.array([[x * (res - 1), y * (res - 1)] for x, y in points], np.float32)
    cv2.fillPoly(img, [np.round(pts * 16).astype(np.int32)], 255, cv2.LINE_AA, shift=4)
    return img


def _ellipse(res=TEMPLATE_RES) -> np.ndarray:
    img = np.zeros((res, res), np.uint8)
    c = (res - 1) / 2
    cv2.ellipse(img, ((c, c), (res - 1, res - 1), 0), 255, -1, cv2.LINE_AA)
    return img


def _star(n=5, inner=0.45):
    pts = []
    for i in range(2 * n):
        r = 0.5 if i % 2 == 0 else 0.5 * inner
        a = -math.pi / 2 + i * math.pi / n
        pts.append((0.5 + r * math.cos(a), 0.5 + r * math.sin(a)))
    return pts


def _regular(n, phase=0.0):
    return [(0.5 + 0.5 * math.cos(phase + 2 * math.pi * i / n), 0.5 + 0.5 * math.sin(phase + 2 * math.pi * i / n)) for i in range(n)]


def _heart(res=TEMPLATE_RES) -> np.ndarray:
    t = np.linspace(0, 2 * np.pi, 400)
    x = 16 * np.sin(t) ** 3
    y = -(13 * np.cos(t) - 5 * np.cos(2 * t) - 2 * np.cos(3 * t) - np.cos(4 * t))
    x = (x - x.min()) / (x.max() - x.min())
    y = (y - y.min()) / (y.max() - y.min())
    return _poly(list(zip(x, y)), res)


def _rotate_crop(img: np.ndarray, angle: float) -> np.ndarray:
    if angle % 360 == 0:
        out = img
    else:
        h, w = img.shape
        d = int(math.ceil(math.hypot(h, w)))
        pad = np.zeros((d, d), np.uint8)
        y0, x0 = (d - h) // 2, (d - w) // 2
        pad[y0:y0 + h, x0:x0 + w] = img
        m = cv2.getRotationMatrix2D((d / 2, d / 2), angle, 1.0)
        out = cv2.warpAffine(pad, m, (d, d), flags=cv2.INTER_LINEAR)
    ys, xs = np.nonzero(out > 127)
    return out[ys.min():ys.max() + 1, xs.min():xs.max() + 1]


ROTATIONS = (0, 45, 90, 135, 180, 225, 270, 315)


@functools.cache
def _fixed_templates() -> tuple[tuple[str, str, str, np.ndarray], ...]:
    """(class, template name, aspect gate, cropped image) for aspect-independent templates."""
    arrows = {}
    for hh in (0.4, 0.55, 0.7):
        for sw in (0.3, 0.45, 0.6):
            arrows[f"stem_arrow_h{hh}_s{sw}"] = [(0.5, 0), (1, hh), (0.5 + sw / 2, hh), (0.5 + sw / 2, 1), (0.5 - sw / 2, 1), (0.5 - sw / 2, hh), (0, hh)]
    arrows["triangle"] = [(0.5, 0), (1, 1), (0, 1)]
    for th in (0.2, 0.35, 0.5):
        arrows[f"chevron_t{th}"] = [(0.5, 0), (1, 0.5), (1, 0.5 + th), (0.5, th), (0, 0.5 + th), (0, 0.5)]
    for n in (0.25, 0.45):
        arrows[f"dart_n{n}"] = [(0.5, 0), (1, 1), (0.5, 1 - n), (0, 1)]
    # DDR/StepMania arrows: a 90-degree head whose arms end in square cuts, plus a stem.
    for c in (0.12, 0.2, 0.28):
        for sw in (0.3, 0.45):
            y = 2 * c + sw / 2
            arrows[f"ddr_arrow_c{c}_s{sw}"] = [(0.5, 0), (1, 0.5), (1 - c, 0.5 + c), (0.5 + sw / 2, y), (0.5 + sw / 2, 1), (0.5 - sw / 2, 1), (0.5 - sw / 2, y), (c, 0.5 + c), (0, 0.5)]

    etc_polys = {
        "diamond": [(0.5, 0), (1, 0.5), (0.5, 1), (0, 0.5)],
        "star5": _star(5),
        "star4": _star(4, 0.4),
        "plus": [(0.33, 0), (0.67, 0), (0.67, 0.33), (1, 0.33), (1, 0.67), (0.67, 0.67), (0.67, 1), (0.33, 1), (0.33, 0.67), (0, 0.67), (0, 0.33), (0.33, 0.33)],
        "hexagon_flat": _regular(6),
        "hexagon_pointy": _regular(6, math.pi / 2),
        "octagon": _regular(8, math.pi / 8),
    }
    out = []
    for name, pts in arrows.items():
        base = _poly(pts)
        for r in ROTATIONS:
            out.append(("arrow", f"{name}@{r}", "any", _rotate_crop(base, r)))
    out.append(("circle", "ellipse", "round", _rotate_crop(_ellipse(), 0)))
    for name, pts in etc_polys.items():
        base = _poly(pts)
        gate = "any" if name in ("diamond", "star5", "star4", "plus") else "compact"
        rots = (0, 45) if name in ("star4", "plus") else (0,)
        for r in rots:
            out.append(("etc", f"{name}@{r}", gate, _rotate_crop(base, r)))
    out.append(("etc", "heart", "compact", _rotate_crop(_heart(), 0)))
    return tuple(out)


def _round_rect(w: int, h: int, radius: float) -> np.ndarray:
    img = np.zeros((h, w), np.uint8)
    r = int(round(min(radius, w / 2, h / 2)))
    if r <= 0:
        img[:] = 255
        return img
    cv2.rectangle(img, (r, 0), (w - 1 - r, h - 1), 255, -1)
    cv2.rectangle(img, (0, r), (w - 1, h - 1 - r), 255, -1)
    for cx, cy in ((r, r), (w - 1 - r, r), (r, h - 1 - r), (w - 1 - r, h - 1 - r)):
        cv2.circle(img, (cx, cy), r, 255, -1, cv2.LINE_AA)
    return img


def _aspect_templates(w: int, h: int) -> list[tuple[str, str, str, np.ndarray]]:
    """Templates whose geometry depends on the sample's aspect ratio (corner radii, end caps)."""
    short = min(w, h)
    out = [
        ("bar", "rect", "elongated", _round_rect(w, h, 0)),
        ("bar", "round_rect_0.2", "elongated", _round_rect(w, h, 0.2 * short)),
        ("bar", "stadium", "elongated", _round_rect(w, h, 0.5 * short)),
        ("etc", "square", "compact", _round_rect(w, h, 0)),
        ("etc", "round_square", "compact", _round_rect(w, h, 0.2 * short)),
    ]
    for name, inset in (("chamfer_bar", 0.3), ("pointed_bar", 0.5)):
        c = inset * short  # corner cut in pixels along both edges
        if w >= h:
            pts = [(c, 0), (w - 1 - c, 0), (w - 1, c), (w - 1, h - 1 - c), (w - 1 - c, h - 1), (c, h - 1), (0, h - 1 - c), (0, c)]
        else:
            pts = [(0, c), (c, 0), (w - 1 - c, 0), (w - 1, c), (w - 1, h - 1 - c), (w - 1 - c, h - 1), (c, h - 1), (0, h - 1 - c)]
        img = np.zeros((h, w), np.uint8)
        cv2.fillPoly(img, [np.round(np.array(pts, np.float32) * 16).astype(np.int32)], 255, cv2.LINE_AA, shift=4)
        out.append(("bar", name, "elongated", img))
    return out


def opaque_mask(rgba: np.ndarray, t: Thresholds) -> tuple[np.ndarray, str]:
    alpha = rgba[:, :, 3]
    amax = int(alpha.max()) if alpha.size else 0
    if amax == 0:
        return np.zeros(alpha.shape, bool), "transparent"
    if int(alpha.min()) < 250:
        thr = max(t.alpha_floor, t.alpha_rel * amax)
        return alpha >= thr, "alpha"
    # Fully opaque image (e.g. JPEG): separate the sprite from a uniform border colour.
    rgb = rgba[:, :, :3].astype(np.int16)
    border = np.concatenate([rgb[0], rgb[-1], rgb[:, 0], rgb[:, -1]])
    bg = np.median(border, axis=0)
    spread = np.abs(border - bg).max(axis=1)
    if np.mean(spread <= t.opaque_bg_tolerance) < 0.9:
        return np.ones(alpha.shape, bool), "opaque-rect"
    fg = np.abs(rgb - bg).max(axis=2) > t.opaque_bg_tolerance
    if fg.mean() < 0.01:
        return np.ones(alpha.shape, bool), "opaque-rect"
    return fg, "opaque-bg"


def _silhouette(mask: np.ndarray, t: Thresholds) -> tuple[np.ndarray | None, dict]:
    ys, xs = np.nonzero(mask)
    if len(xs) == 0:
        return None, {}
    m = mask[ys.min():ys.max() + 1, xs.min():xs.max() + 1].astype(np.uint8) * 255
    h, w = m.shape
    scale = WORK / max(h, w)
    nw, nh = max(1, round(w * scale)), max(1, round(h * scale))
    m = cv2.resize(m, (nw, nh), interpolation=cv2.INTER_AREA if scale < 1 else cv2.INTER_LINEAR)
    m = (m > 127).astype(np.uint8) * 255
    # Drop specks (sparkles, debug labels) before closing so they cannot fuse onto the shape.
    n0, labels0, stats0, _ = cv2.connectedComponentsWithStats(m, connectivity=8)
    if n0 > 2:
        areas0 = stats0[1:, cv2.CC_STAT_AREA]
        small = [i + 1 for i, a in enumerate(areas0) if a < t.component_min_frac * areas0.max()]
        m[np.isin(labels0, small)] = 0
    k = max(1, round(t.close_frac * WORK))
    m = cv2.morphologyEx(cv2.copyMakeBorder(m, k, k, k, k, cv2.BORDER_CONSTANT, 0), cv2.MORPH_CLOSE, cv2.getStructuringElement(cv2.MORPH_ELLIPSE, (2 * k + 1, 2 * k + 1)))
    n, labels, stats, _ = cv2.connectedComponentsWithStats(m, connectivity=8)
    if n <= 1:
        return None, {}
    areas = stats[1:, cv2.CC_STAT_AREA]
    largest = areas.max()
    keep = [i + 1 for i, a in enumerate(areas) if a >= t.component_min_frac * largest]
    sig = np.isin(labels, keep).astype(np.uint8) * 255
    contours, _ = cv2.findContours(sig, cv2.RETR_EXTERNAL, cv2.CHAIN_APPROX_NONE)
    filled = np.zeros_like(sig)
    cv2.drawContours(filled, contours, -1, 255, -1)
    ys, xs = np.nonzero(filled)
    filled = filled[ys.min():ys.max() + 1, xs.min():xs.max() + 1]
    total = sum(int(stats[i, cv2.CC_STAT_AREA]) for i in keep)
    feats = {
        "components": len(keep),
        "largest_component_frac": round(float(largest) / total, 3),
    }
    return filled, feats


def _gate_ok(gate: str, aspect: float, t: Thresholds) -> bool:
    long_short = max(aspect, 1 / aspect)
    if gate == "round":
        return long_short <= t.circle_max_aspect
    if gate == "elongated":
        return long_short >= t.bar_min_aspect
    if gate == "compact":
        return long_short < t.bar_min_aspect
    return True


def classify_mask(mask: np.ndarray, t: Thresholds) -> ShapeResult:
    sil, feats = _silhouette(mask, t)
    if sil is None:
        return ShapeResult("unknown", 0.0, "empty sprite", {})
    h, w = sil.shape
    if min(mask.shape) < 2 or max(h, w) < 8:
        return ShapeResult("unknown", 0.0, "sprite too small", feats)
    aspect = w / h
    sb = sil > 127
    area = sb.sum()
    contours, _ = cv2.findContours(sil, cv2.RETR_EXTERNAL, cv2.CHAIN_APPROX_NONE)
    hull_area = sum(cv2.contourArea(cv2.convexHull(c)) for c in contours) or 1
    rot = sb[::-1, ::-1]
    point_symmetry = float(np.logical_and(sb, rot).sum()) / max(1, np.logical_or(sb, rot).sum())
    feats.update({
        "aspect": round(aspect, 3),
        "extent": round(float(area) / (h * w), 3),
        "solidity": round(float(area) / hull_area, 3),
        "point_symmetry": round(point_symmetry, 3),
    })

    best: dict[str, tuple[float, str]] = {}
    templates = list(_fixed_templates()) + _aspect_templates(w, h)
    for cls, name, gate, tpl in templates:
        if not _gate_ok(gate, aspect, t):
            continue
        # An arrow has one mirror axis; a shape that maps onto itself when turned 180 degrees is not one.
        if cls == "arrow" and point_symmetry >= t.arrow_max_point_symmetry:
            continue
        if tpl.shape != (h, w):
            tpl = cv2.resize(tpl, (w, h), interpolation=cv2.INTER_AREA)
        tb = tpl > 127
        inter = np.logical_and(sb, tb).sum()
        union = np.logical_or(sb, tb).sum()
        iou = float(inter) / union if union else 0.0
        if iou > best.get(cls, (-1.0, ""))[0]:
            best[cls] = (iou, name)

    ranked = sorted(best.items(), key=lambda kv: kv[1][0], reverse=True)
    feats["iou"] = {cls: round(v[0], 3) for cls, v in ranked}
    feats["best_template"] = {cls: v[1] for cls, v in ranked}
    top_cls, (top_iou, top_name) = ranked[0]
    if (
        top_cls == "etc"
        and top_name.split("@")[0] in POINT_SYMMETRIC
        and point_symmetry < t.arrow_max_point_symmetry
        and best.get("arrow", (0.0, ""))[0] >= t.match_min_iou
    ):
        # The silhouette breaks the template's 180-degree symmetry (e.g. a diamond-bodied arrow with
        # a stem stub), so the near-tied arrow match is the better explanation.
        ranked = [("arrow", best["arrow"])] + [kv for kv in ranked if kv[0] != "arrow"]
        top_cls, (top_iou, top_name) = ranked[0]
    second_iou = ranked[1][1][0] if len(ranked) > 1 else 0.0

    if feats["largest_component_frac"] < t.fragment_min_frac:
        return ShapeResult("etc", round(1 - feats["largest_component_frac"], 3), "fragmented sprite (multiple separate parts)", feats)
    if top_iou < t.match_min_iou:
        return ShapeResult("etc", round(1 - top_iou, 3), f"no template fits (best {top_cls} {top_iou:.2f})", feats)
    margin = top_iou - second_iou
    if top_cls != "etc" and len(ranked) > 1 and ranked[1][0] != "etc" and margin < t.ambiguity_margin:
        return ShapeResult("unknown", round(margin, 3), f"ambiguous between {top_cls} and {ranked[1][0]}", feats)
    return ShapeResult(top_cls, round(min(1.0, top_iou), 3), f"matched {top_name}", feats)
