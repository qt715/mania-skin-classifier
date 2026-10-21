"""Cut amount of an LN body: background rows above where the body starts (Percy "d").

"Background" covers the three forms seen in real Percy skins:
- fully transparent rows (the common case),
- a faint translucent "ghost" of the true LN end (alpha far below the body's own alpha),
- an opaque uniform cover band (e.g. solid black) that differs from the body itself.
Small decorations above the body (debug labels, sparkles) are skipped: the body starts at the
first non-background run of at least ``body_min_run_px`` logical pixels.
"""

from __future__ import annotations

import numpy as np

from .config import Thresholds


def _uniform_band(top: np.ndarray, t: Thresholds) -> int:
    """Number of leading rows forming an opaque, uniform cover band unlike the deep body."""
    if top.shape[0] < 2:
        return 0
    first = top[0].astype(np.int16)
    vis = first[:, 3] > t.blank_alpha
    if vis.mean() < 0.9:
        return 0
    colour = np.median(first[vis], axis=0)
    if np.abs(first[vis] - colour).max() > t.band_colour_tol:
        return 0
    deep = top[-1].astype(np.int16)
    deep_vis = deep[:, 3] > t.blank_alpha
    if deep_vis.mean() >= 0.9 and np.abs(deep[deep_vis] - colour).max() <= t.band_colour_tol:
        return 0  # the "band" is just a uniform body
    n = 0
    for row in top:
        r = row.astype(np.int16)
        v = r[:, 3] > t.blank_alpha
        if v.mean() < 0.9 or np.abs(r[v] - colour).max() > t.band_colour_tol:
            break
        n += 1
    return n


def cut_amount(rows: np.ndarray, top: np.ndarray, scale: int, t: Thresholds) -> dict:
    h = int(rows.size)
    if h == 0:
        return {"top_blank_px": 0.0, "body_start_row": 0, "empty": True}
    lower = rows[h // 2:] if h >= 2 else rows
    ref = float(np.percentile(lower, 90))
    ghost = max(t.blank_alpha, t.ghost_alpha_rel * ref)
    bg = rows <= ghost
    band = _uniform_band(top, t)
    bg[:band] = True

    min_run = max(1, int(np.ceil(t.body_min_run_px * scale)))
    fg_idx = np.flatnonzero(~bg)
    start = None
    if fg_idx.size:
        # Split foreground rows into contiguous runs; pick the first long one (or one reaching the bottom).
        breaks = np.flatnonzero(np.diff(fg_idx) > 1)
        run_starts = np.concatenate(([fg_idx[0]], fg_idx[breaks + 1]))
        run_ends = np.concatenate((fg_idx[breaks], [fg_idx[-1]])) + 1
        for s, e in zip(run_starts, run_ends):
            if e - s >= min_run or e == h:
                start = int(s)
                break
    if start is None:
        return {"top_blank_px": h / scale, "body_start_row": h, "empty": True, "ghost_alpha": round(ghost, 1), "cover_band_rows": band}
    return {
        "top_blank_px": round(start / scale, 1),
        "body_start_row": start,
        "empty": False,
        "ghost_alpha": round(ghost, 1),
        "cover_band_rows": band,
    }
