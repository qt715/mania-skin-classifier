"""Per-skin pipeline: parse skin.ini, resolve per-column sprites, classify, aggregate per key mode."""

from __future__ import annotations

from dataclasses import dataclass

from . import __version__
from .assets import Asset, fallback_index, resolve_column, resolve_tail
from .config import Thresholds
from .imageio import ImageError, body_rows, header, load_rgba
from .ln import cut_amount
from .shapes import SHAPES, classify_mask, opaque_mask
from .skinini import parse_skin_ini
from .sources import SkinSource

SCHEMA_VERSION = 1


@dataclass
class _Cache:
    """Results keyed by file content hash: identical images are decoded and analysed once."""

    shapes: dict
    profiles: dict


def _note_shape(source: SkinSource, asset: Asset, t: Thresholds, cache: _Cache) -> dict:
    path = asset.first.path
    digest = source.sha256(path)
    if digest not in cache.shapes:
        try:
            rgba, stride = load_rgba(source.read(path))
            mask, mask_kind = opaque_mask(rgba, t)
            r = classify_mask(mask, t)
            cache.shapes[digest] = {
                "shape": r.shape,
                "confidence": r.confidence,
                "reason": r.reason,
                "mask": mask_kind,
                "size": [int(rgba.shape[1] * stride), int(rgba.shape[0] * stride)],
                "features": r.features,
            }
        except ImageError as e:
            cache.shapes[digest] = {"shape": "unknown", "confidence": 0.0, "reason": str(e)}
    return {"sha256": digest, **cache.shapes[digest]}


def _profile(source: SkinSource, path: str, t: Thresholds, cache: _Cache) -> dict:
    digest = source.sha256(path)
    if digest not in cache.profiles:
        try:
            data = source.read(path)
            hd = header(data)
            rows, top = body_rows(data, t.top_scan_rows)
            cache.profiles[digest] = {"width": hd.width, "height": hd.height, "rows": rows, "top": top}
        except ImageError as e:
            cache.profiles[digest] = {"error": str(e)}
    return {"sha256": digest, **cache.profiles[digest]}


def _int_style(raw: str | None) -> int | None:
    if raw is None or raw == "":
        return None
    try:
        return int(raw)
    except ValueError:
        return None


def _ln_column(source: SkinSource, values: dict[str, str], keys: int, col: int, t: Thresholds, cache: _Cache, unresolved: list) -> dict:
    col_style = _int_style(values.get(f"NoteBodyStyle{col}"))
    sec_style = _int_style(values.get("NoteBodyStyle"))
    if col_style is not None:
        style, style_from = col_style, f"NoteBodyStyle{col}"
    elif sec_style is not None:
        style, style_from = sec_style, "NoteBodyStyle"
    else:
        style, style_from = 1, "default"

    body_name, body = resolve_column(source, values, keys, col, "L")
    ev: dict = {"body_style": style, "body_style_from": style_from, "body_lookup": body_name}
    if body is None:
        unresolved.append({"key_mode": keys, "column": col, "role": "ln_body", "lookup": body_name, "configured": f"NoteImage{col}L" in values})
        return {"cut": "unknown", "reason": "LN body unresolved", **ev}

    prof = _profile(source, body.first.path, t, cache)
    ev.update({"body_file": source.original_path(body.first.path), "body_sha256": prof["sha256"], "body_frames": len(body.frames)})
    if "error" in prof:
        return {"cut": "unknown", "reason": prof["error"], **ev}

    scale = body.first.scale
    logical_h = prof["height"] / scale
    cut = cut_amount(prof["rows"], prof["top"], scale, t)
    ev.update({
        "body_size": [prof["width"], prof["height"]],
        "body_scale": scale,
        "body_logical_height": round(logical_h, 1),
        "body_top_blank_px": cut["top_blank_px"],
        "body_cut_analysis": {k: v for k, v in cut.items() if k != "top_blank_px"},
    })

    tail_role, tail_name, tail = resolve_tail(source, values, keys, col)
    tail_transparent = None
    if tail is not None:
        tprof = _profile(source, tail.first.path, t, cache)
        if "error" not in tprof:
            tail_transparent = bool(tprof["rows"].size == 0 or int(tprof["rows"].max()) <= t.tail_transparent_alpha)
        ev.update({"tail_role": tail_role, "tail_file": source.original_path(tail.first.path), "tail_transparent": tail_transparent})
    else:
        ev.update({"tail_role": None, "tail_lookup": tail_name, "tail_transparent": None})

    if cut["empty"]:
        return {"cut": "no", "reason": "LN body has no visible content", **ev}
    checks = {
        "style": style == t.percy_body_style,
        "tall": logical_h >= t.percy_min_height,
        "top_blank": cut["top_blank_px"] >= t.percy_min_top_blank_px,
    }
    ev["checks"] = checks
    if all(checks.values()):
        conf = 0.95 if tail_transparent else 0.75
        why = "tall body with blank top" + (", transparent tail" if tail_transparent else ", tail not transparent")
        return {"cut": "yes", "confidence": conf, "reason": why, **ev}
    failed = ", ".join(k for k, ok in checks.items() if not ok)
    return {"cut": "no", "reason": f"fails: {failed}", **ev}


def _aggregate_cut(cols: list[str]) -> str:
    has_yes, has_no = "yes" in cols, "no" in cols
    if has_yes and has_no:
        return "mixed"
    if "unknown" in cols:
        return "unknown"
    return "all" if has_yes else "none"


def classify_skin(source: SkinSource, t: Thresholds, details: bool = False, content_hash: bool = True) -> dict:
    out: dict = {
        "schema_version": SCHEMA_VERSION,
        "tool": {"name": "mania-skin-classifier", "version": __version__},
        "skin": {"input": source.location, "kind": source.kind, "name": source.name},
        "key_modes": {},
        "unresolved": [],
        "warnings": [],
    }
    if content_hash:
        out["skin"]["content_hash"] = source.content_hash()
    detail_modes: dict = {}
    if details:
        out["thresholds"] = t.to_dict()
        out["details"] = detail_modes  # filled in place below
    if not source.has("skin.ini"):
        out["warnings"].append("no skin.ini; no declared mania key modes")
        return out

    ini = parse_skin_ini(source.read("skin.ini"))
    out["warnings"].extend(ini.warnings)
    if ini.general.get("Name"):
        out["skin"]["name"] = ini.general["Name"]
    out["skin"]["ini_version"] = ini.version

    cache = _Cache({}, {})
    for keys in sorted(ini.mania):
        values = ini.mania[keys].values
        cols_detail = []
        shapes: set[str] = set()
        unclassified: list[int] = []
        cuts: list[str] = []
        for col in range(keys):
            note_name, note = resolve_column(source, values, keys, col, "")
            cd: dict = {"column": col, "fallback": fallback_index(keys, col), "note_lookup": note_name}
            if note is None:
                out["unresolved"].append({"key_mode": keys, "column": col, "role": "note", "lookup": note_name, "configured": f"NoteImage{col}" in values})
                cd["note"] = {"shape": "unresolved"}
                unclassified.append(col)
            else:
                ns = _note_shape(source, note, t, cache)
                cd["note"] = {"file": source.original_path(note.first.path), "scale": note.first.scale, "frames": len(note.frames), **ns}
                if ns["shape"] in SHAPES:
                    shapes.add(ns["shape"])
                else:
                    unclassified.append(col)
            ln = _ln_column(source, values, keys, col, t, cache, out["unresolved"])
            cd["ln"] = ln
            cuts.append(ln["cut"])
            cols_detail.append(cd)

        out["key_modes"][str(keys)] = {
            "note_shapes": [s for s in SHAPES if s in shapes],
            "ln_cut": _aggregate_cut(cuts),
            "unclassified_columns": unclassified,
        }
        detail_modes[str(keys)] = cols_detail
    return out
