import numpy as np
import pytest
from conftest import chevron_up, circle, diamond, percy_body, ring, rgba, rounded_bar, square, stem_arrow_left

from mania_skin_classifier.classify import classify_skin
from mania_skin_classifier.config import Thresholds
from mania_skin_classifier.shapes import classify_mask, opaque_mask
from mania_skin_classifier.sources import DirSource

T = Thresholds()


@pytest.mark.parametrize(
    ("sprite", "expected"),
    [
        (circle(), "circle"),
        (ring(), "circle"),
        (stem_arrow_left(), "arrow"),
        (chevron_up(), "arrow"),
        (rounded_bar(), "bar"),
        (diamond(), "etc"),
        (square(), "etc"),
        (rgba(64, 64), "unknown"),
    ],
)
def test_shape_classes(sprite, expected):
    mask, _ = opaque_mask(sprite, T)
    assert classify_mask(mask, T).shape == expected


def test_opaque_sprite_on_uniform_background_uses_foreground():
    img = circle()
    a = img[:, :, 3:4].astype(np.float32) / 255
    img[:, :, :3] = (img[:, :, :3] * a).astype(np.uint8)  # composite onto black, like a JPEG
    img[:, :, 3] = 255
    mask, kind = opaque_mask(img, T)
    assert kind == "opaque-bg"
    assert classify_mask(mask, T).shape == "circle"


def _ln_skin(make_skin, style_line: str, bodies: dict, tail_transparent=True):
    files = {"mania-note1.png": circle(), "mania-note2.png": circle()}
    files.update(bodies)
    tail = rgba(64, 32) if tail_transparent else circle(64)
    files["mania-note1T.png"] = tail
    files["mania-note2T.png"] = tail
    return DirSource(make_skin(f"[Mania]\nKeys: 4\n{style_line}\n", files))


def test_percy_bodies_in_every_column_are_cut(make_skin):
    src = _ln_skin(make_skin, "", {"mania-note1L.png": percy_body(), "mania-note2L.png": percy_body()})
    r = classify_skin(src, T, content_hash=False)
    assert r["key_modes"]["4"] == {"note_shapes": ["circle"], "ln_cut": "all", "unclassified_columns": []}


def test_stretch_style_is_not_cut(make_skin):
    src = _ln_skin(make_skin, "NoteBodyStyle: 0", {"mania-note1L.png": percy_body(), "mania-note2L.png": percy_body()})
    assert classify_skin(src, T, content_hash=False)["key_modes"]["4"]["ln_cut"] == "none"


def test_per_column_style_override_makes_mode_mixed(make_skin):
    src = _ln_skin(make_skin, "NoteBodyStyle: 1\nNoteBodyStyle0: 0", {"mania-note1L.png": percy_body(), "mania-note2L.png": percy_body()})
    assert classify_skin(src, T, content_hash=False)["key_modes"]["4"]["ln_cut"] == "mixed"


def test_2x_body_height_is_normalized(make_skin):
    # 1800 px @2x is 900 logical px: below the 1000 px Percy minimum.
    src = _ln_skin(make_skin, "", {"mania-note1L@2x.png": percy_body(height=1800, blank=100), "mania-note2L@2x.png": percy_body(height=1800, blank=100)})
    assert classify_skin(src, T, content_hash=False)["key_modes"]["4"]["ln_cut"] == "none"


def test_tall_body_without_blank_top_is_not_cut(make_skin):
    src = _ln_skin(make_skin, "", {"mania-note1L.png": percy_body(blank=0), "mania-note2L.png": percy_body(blank=0)})
    assert classify_skin(src, T, content_hash=False)["key_modes"]["4"]["ln_cut"] == "none"


def test_ghost_end_and_cover_band_count_as_background(make_skin):
    ghost = percy_body(blank=0)
    ghost[:60, 8:56] = (200, 200, 200, 30)  # faint preview of the real LN end
    band = percy_body(blank=0)
    band[:70, :] = (0, 0, 0, 255)  # opaque black cover
    src = _ln_skin(make_skin, "", {"mania-note1L.png": ghost, "mania-note2L.png": band})
    r = classify_skin(src, T, details=True, content_hash=False)
    blanks = [c["ln"]["body_top_blank_px"] for c in r["details"]["4"][:2]]
    assert blanks == [60.0, 70.0]
    assert r["key_modes"]["4"]["ln_cut"] == "all"


def test_missing_body_is_unknown_and_reported(make_skin):
    src = _ln_skin(make_skin, "", {"mania-note1L.png": percy_body()})
    r = classify_skin(src, T, content_hash=False)
    assert r["key_modes"]["4"]["ln_cut"] == "unknown"
    assert {u["column"] for u in r["unresolved"] if u["role"] == "ln_body"} == {1, 2}


def test_huge_body_is_streamed(make_skin, monkeypatch):
    import mania_skin_classifier.imageio as io_mod

    monkeypatch.setattr(io_mod, "STREAM_PIXELS", 10_000)
    src = _ln_skin(make_skin, "", {"mania-note1L.png": percy_body(), "mania-note2L.png": percy_body()})
    r = classify_skin(src, T, details=True, content_hash=False)
    assert r["key_modes"]["4"]["ln_cut"] == "all"
    assert r["details"]["4"][0]["ln"]["body_top_blank_px"] == 50.0


def test_unresolved_notes_are_unclassified_not_guessed(make_skin):
    src = DirSource(make_skin("[Mania]\nKeys: 2\nNoteImage1: nope\n", {"mania-note1.png": circle()}))
    r = classify_skin(src, T, content_hash=False)
    assert r["key_modes"]["2"]["note_shapes"] == ["circle"]
    assert r["key_modes"]["2"]["unclassified_columns"] == [1]


def test_skin_without_mania_section_has_no_key_modes(make_skin):
    src = DirSource(make_skin("[General]\nName: osu only\n", {"mania-note1.png": circle()}))
    assert classify_skin(src, T, content_hash=False)["key_modes"] == {}


def test_content_hash_ignores_path_case_and_source_root(make_skin, tmp_path):
    a = make_skin("[Mania]\nKeys: 1\n", {"Mania-NoteS.png": circle()})
    nested = tmp_path / "wrap" / "inner"
    nested.mkdir(parents=True)
    for p in a.iterdir():
        (nested / p.name.lower()).write_bytes(p.read_bytes())
    assert DirSource(a).content_hash() == DirSource(tmp_path / "wrap").content_hash()
