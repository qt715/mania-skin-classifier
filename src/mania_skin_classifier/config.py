from __future__ import annotations

import json
from dataclasses import asdict, dataclass, fields
from pathlib import Path


@dataclass(frozen=True)
class Thresholds:
    # --- note silhouette extraction ---
    alpha_floor: int = 24  # minimum alpha counted as part of the sprite
    alpha_rel: float = 0.45  # ...and at least this fraction of the sprite's peak alpha (drops soft glows)
    opaque_bg_tolerance: int = 40  # per-channel distance from the border colour for opaque sprites
    close_frac: float = 0.02  # morphological closing radius, fraction of the normalized size
    component_min_frac: float = 0.05  # ignore specks smaller than this fraction of the largest part
    # --- note shape decision ---
    fragment_min_frac: float = 0.6  # largest part must hold this share of the sprite, else etc
    circle_max_aspect: float = 1.35  # long/short side ratio allowed for circle
    bar_min_aspect: float = 1.3  # long/short side ratio required for bar (and the limit for squares)
    match_min_iou: float = 0.8  # below this, no template fits and the sprite is etc
    ambiguity_margin: float = 0.02  # top two of circle/arrow/bar closer than this -> unknown
    arrow_max_point_symmetry: float = 0.93  # 180-degree self-IoU at or above this rules out arrow
    # --- cut (Percy) LN detection ---
    percy_body_style: int = 1
    percy_min_height: float = 1000.0  # logical (@2x-normalized) LN body height in px
    # Cut amount d = logical px of background above the body (Percy Skin Editor's "d").
    # Absolute, not a fraction: real Percy bodies are 20k-40k px tall with d of 20-160 px.
    percy_min_top_blank_px: float = 4.0
    blank_alpha: int = 8  # a row whose maximum alpha is at most this is transparent
    ghost_alpha_rel: float = 0.25  # rows below this share of the body's alpha are a faint "ghost" end preview
    body_min_run_px: float = 32.0  # the body starts at the first non-background run at least this long
    band_colour_tol: int = 8  # per-channel tolerance for an opaque uniform cover band at the top
    top_scan_rows: int = 4096  # rows decoded at full resolution for the cut analysis
    tail_transparent_alpha: int = 8  # tail whose maximum alpha is at most this is transparent

    @classmethod
    def load(cls, path: Path | None) -> Thresholds:
        if path is None:
            return cls()
        data = json.loads(path.read_text())
        known = {f.name for f in fields(cls)}
        unknown = set(data) - known
        if unknown:
            raise ValueError(f"unknown threshold(s): {', '.join(sorted(unknown))}")
        return cls(**data)

    def to_dict(self) -> dict:
        return asdict(self)
