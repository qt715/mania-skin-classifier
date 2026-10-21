from mania_skin_classifier.skinini import parse_skin_ini


def test_repeated_mania_sections_each_define_a_key_mode():
    ini = parse_skin_ini(b"[General]\nName: x\n\n[Mania]\nKeys: 4\nNoteImage0: a\n\n[Mania]\nKeys: 7\nNoteImage0: b\n")
    assert sorted(ini.mania) == [4, 7]
    assert ini.mania[4].values["NoteImage0"] == "a"
    assert ini.mania[7].values["NoteImage0"] == "b"


def test_lines_before_keys_belong_to_that_section():
    ini = parse_skin_ini(b"[Mania]\nNoteBodyStyle: 2\nKeys: 4\n")
    assert ini.mania[4].values["NoteBodyStyle"] == "2"


def test_duplicate_key_count_first_section_wins():
    ini = parse_skin_ini(b"[Mania]\nKeys: 4\nNoteImage0: first\n[Mania]\nKeys: 4\nNoteImage0: second\n")
    assert ini.mania[4].values["NoteImage0"] == "first"
    assert any("duplicate" in w for w in ini.warnings)


def test_undeclared_and_invalid_key_counts_are_not_modes():
    ini = parse_skin_ini(b"[Mania]\nKeys: 11\nNoteImage0: x\n[Mania]\nKeys: abc\n[Mania]\nNoteImage0: orphan\n")
    assert ini.mania == {}
    assert len(ini.warnings) == 2


def test_field_and_section_names_are_case_sensitive():
    ini = parse_skin_ini(b"[mania]\nKeys: 4\n[Mania]\nkeys: 5\nKeys: 6\nnoteimage0: lower\nNoteImage0: upper\n")
    assert sorted(ini.mania) == [6]
    assert ini.mania[6].values["NoteImage0"] == "upper"


def test_comments_and_bom():
    ini = parse_skin_ini("\ufeff[Mania]\n// Keys: 5\nKeys: 4 // four\nNoteImage0: notes\\n0\n".encode())
    assert sorted(ini.mania) == [4]
    assert ini.mania[4].values["NoteImage0"] == "notes\\n0"
