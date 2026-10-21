"""Texture name resolution matching osu!lazer's legacy skin lookups.

Sources (ppy/osu master, 2026-10):
- LegacyManiaColumnElement: fallback column index "S" for the special column, otherwise
  "1"/"2" alternating by distance to the nearest stage edge.
- StageDefinition.IsSpecialColumn: odd key count and the exact middle column.
- LegacyNotePiece / LegacyBodyPiece / LegacyHoldNoteTailPiece: ``NoteImage#``/``#H``/``#L``/``#T``
  else ``mania-note{1,2,S}{,H,L,T}``; the tail falls back to head, then note.
- LegacySkin.GetTexture: strips ``@2x`` from names, prefers ``name@2x`` (scale 2) over ``name``.
- LegacySkinExtensions.GetTextures: animations use ``name-0``, ``name-1``... when ``name-0`` exists.
- RealmBackedResourceStore: lookups are case-insensitive with ``\\`` normalized to ``/``.
"""

from __future__ import annotations

from dataclasses import dataclass

from .sources import SkinSource, normalize

IMAGE_EXTENSIONS = (".png", ".jpg", ".jpeg")


@dataclass(frozen=True)
class Frame:
    path: str  # normalized path inside the skin
    scale: int  # 2 for @2x assets


@dataclass(frozen=True)
class Asset:
    lookup: str  # the texture name that was looked up
    configured: bool  # True if named by a skin.ini NoteImage# entry
    frames: tuple[Frame, ...]

    @property
    def first(self) -> Frame:
        return self.frames[0]


def is_special_column(keys: int, column: int) -> bool:
    # SpecialStyle (even key counts) is deliberately not applied: lazer's texture fallback
    # ignores it, and stable's exact even-key layout is not publicly documented.
    return keys % 2 == 1 and column == keys // 2


def fallback_index(keys: int, column: int) -> str:
    if is_special_column(keys, column):
        return "S"
    distance_to_edge = min(column, keys - 1 - column)
    return "1" if distance_to_edge % 2 == 0 else "2"


def _file_candidates(name: str) -> list[str]:
    base = normalize(name)
    stem, dot, ext = base.rpartition(".")
    if dot and "." + ext in IMAGE_EXTENSIONS and "/" not in ext:
        return [base]
    return [base] + [base + e for e in IMAGE_EXTENSIONS]


def _texture(source: SkinSource, name: str) -> Frame | None:
    name = name.replace("@2x", "")
    norm = normalize(name)
    stem, dot, ext = norm.rpartition(".")
    if dot and "." + ext in IMAGE_EXTENSIONS:
        two_x = f"{stem}@2x.{ext}"
    else:
        two_x = f"{norm}@2x"
    for cand in _file_candidates(two_x):
        if source.has(cand):
            return Frame(cand, 2)
    for cand in _file_candidates(norm):
        if source.has(cand):
            return Frame(cand, 1)
    return None


def resolve_texture(source: SkinSource, name: str, configured: bool) -> Asset | None:
    first = _texture(source, f"{name}-0")
    if first is not None:
        frames = [first]
        i = 1
        while (f := _texture(source, f"{name}-{i}")) is not None:
            frames.append(f)
            i += 1
        return Asset(name, configured, tuple(frames))
    single = _texture(source, name)
    return Asset(name, configured, (single,)) if single else None


def column_lookup(values: dict[str, str], keys: int, column: int, suffix: str) -> tuple[str, bool]:
    """Texture name for NoteImage{column}{suffix} or its default, plus whether it was configured."""
    configured = values.get(f"NoteImage{column}{suffix}")
    if configured:
        return configured, True
    return f"mania-note{fallback_index(keys, column)}{suffix}", False


def resolve_column(source: SkinSource, values: dict[str, str], keys: int, column: int, suffix: str) -> tuple[str, Asset | None]:
    name, configured = column_lookup(values, keys, column, suffix)
    return name, resolve_texture(source, name, configured)


def resolve_tail(source: SkinSource, values: dict[str, str], keys: int, column: int) -> tuple[str, str, Asset | None]:
    """Tail texture with lazer's T -> H -> note fallback. Returns (role used, lookup, asset)."""
    last = ""
    for role, suffix in (("tail", "T"), ("head", "H"), ("note", "")):
        name, asset = resolve_column(source, values, keys, column, suffix)
        if asset is not None:
            return role, name, asset
        last = last or name
    return "tail", last, None
