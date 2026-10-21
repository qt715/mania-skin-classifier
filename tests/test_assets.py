from conftest import circle

from mania_skin_classifier.assets import fallback_index, resolve_column, resolve_tail
from mania_skin_classifier.sources import DirSource


def test_default_column_layout_matches_lazer():
    assert [fallback_index(4, c) for c in range(4)] == ["1", "2", "2", "1"]
    assert [fallback_index(7, c) for c in range(7)] == ["1", "2", "1", "S", "1", "2", "1"]
    assert [fallback_index(10, c) for c in range(10)] == ["1", "2", "1", "2", "1", "1", "2", "1", "2", "1"]
    assert fallback_index(1, 0) == "S"


def test_2x_preferred_and_case_insensitive_backslash_paths(make_skin):
    root = make_skin("[Mania]\nKeys: 4\nNoteImage0: Notes\\Note\n", {
        "notes/note.png": circle(),
        "notes/NOTE@2x.png": circle(256),
    })
    _, asset = resolve_column(DirSource(root), {"NoteImage0": "Notes\\Note"}, 4, 0, "")
    assert asset.first.path == "notes/note@2x.png" and asset.first.scale == 2


def test_animation_frames_take_precedence(make_skin):
    root = make_skin("", {"mania-note1.png": circle(), "mania-note1-0.png": circle(), "mania-note1-1.png": circle()})
    _, asset = resolve_column(DirSource(root), {}, 4, 0, "")
    assert [f.path for f in asset.frames] == ["mania-note1-0.png", "mania-note1-1.png"]


def test_configured_but_missing_image_is_unresolved_not_defaulted(make_skin):
    root = make_skin("", {"mania-note1.png": circle()})
    name, asset = resolve_column(DirSource(root), {"NoteImage0": "custom/missing"}, 4, 0, "")
    assert name == "custom/missing" and asset is None


def test_tail_falls_back_to_head_then_note(make_skin):
    root = make_skin("", {"mania-note1.png": circle(), "mania-note1H.png": circle()})
    role, _, asset = resolve_tail(DirSource(root), {}, 4, 0)
    assert role == "head" and asset.first.path == "mania-note1h.png"
