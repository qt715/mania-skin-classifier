"""skin.ini parsing that mirrors osu!lazer's LegacyDecoder / LegacyManiaSkinDecoder line rules.

A conventional INI parser is unusable here: skin.ini repeats ``[Mania]`` headings, field
names are case-sensitive, and a ``Keys:`` line may appear after the fields it governs.
"""

from __future__ import annotations

from dataclasses import dataclass, field

# Key counts listed in the official skin.ini specification.
VALID_KEY_COUNTS = frozenset({1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 12, 14, 16, 18})


@dataclass
class ManiaSection:
    keys: int
    line: int
    values: dict[str, str] = field(default_factory=dict)


@dataclass
class SkinIni:
    general: dict[str, str]
    mania: dict[int, ManiaSection]
    warnings: list[str]

    @property
    def version(self) -> str | None:
        return self.general.get("Version")


def decode_text(data: bytes) -> tuple[str, list[str]]:
    try:
        return data.decode("utf-8-sig"), []
    except UnicodeDecodeError:
        # lazer reads skin.ini as UTF-8; non-UTF-8 bytes become U+FFFD there too, so any file
        # names they spell would not resolve in-game either.
        return data.decode("utf-8-sig", errors="replace"), ["skin.ini is not valid UTF-8; undecodable bytes replaced"]


def _split_key_val(line: str) -> tuple[str, str]:
    key, sep, value = line.partition(":")
    return key.strip(), value.strip() if sep else ""


def parse_skin_ini(data: bytes) -> SkinIni:
    text, warnings = decode_text(data)
    general: dict[str, str] = {}
    mania: dict[int, ManiaSection] = {}
    section: str | None = None
    pending: list[str] = []
    current: ManiaSection | None = None
    current_is_duplicate = False

    def apply(sec: ManiaSection, raw: str) -> None:
        k, v = _split_key_val(raw)
        if k:
            sec.values[k] = v

    for lineno, raw in enumerate(text.splitlines(), 1):
        if not raw.strip() or raw.lstrip().startswith("//"):
            continue
        idx = raw.find("//")
        line = raw[:idx] if idx > 0 else raw
        line = line.rstrip()
        if line.startswith("[") and line.endswith("]"):
            # Section names are matched case-sensitively, like Enum.TryParse in lazer.
            section = line[1:-1]
            pending = []
            current = None
            current_is_duplicate = False
            continue

        if section == "General":
            k, v = _split_key_val(line)
            general[k] = v
        elif section == "Mania":
            k, v = _split_key_val(line)
            if k == "Keys":
                try:
                    keys = int(v)
                except ValueError:
                    warnings.append(f"line {lineno}: unparseable Keys value {v!r}; section ignored")
                    current, current_is_duplicate, pending = None, False, []
                    continue
                if keys not in VALID_KEY_COUNTS:
                    warnings.append(f"line {lineno}: Keys: {keys} is not a valid key count; section ignored")
                    current, current_is_duplicate, pending = None, False, []
                    continue
                current = ManiaSection(keys=keys, line=lineno)
                current_is_duplicate = keys in mania
                if current_is_duplicate:
                    # lazer keeps the first configuration for a key count and silently drops later ones.
                    warnings.append(f"line {lineno}: duplicate [Mania] section for Keys: {keys}; ignored (first wins)")
                else:
                    mania[keys] = current
                for p in pending:
                    apply(current, p)
                pending = []
            elif current is not None:
                apply(current, line)
            else:
                pending.append(line)

    return SkinIni(general=general, mania=mania, warnings=warnings)
