"""Skin file sources: one extracted skin folder or one .osk archive.

Every source exposes the same case-insensitive, forward-slash path namespace that lazer's
RealmBackedResourceStore uses (``filename.ToStandardisedPath().ToLowerInvariant()``).
"""

from __future__ import annotations

import hashlib
import os
import zipfile
from dataclasses import dataclass
from pathlib import Path
from typing import BinaryIO


def normalize(path: str) -> str:
    return path.replace("\\", "/").lstrip("/").lower()


class SkinSource:
    name: str
    location: str
    kind: str

    def paths(self) -> list[str]:
        """Normalized relative paths of all files in the skin."""
        raise NotImplementedError

    def original_path(self, norm: str) -> str:
        raise NotImplementedError

    def open(self, norm: str) -> BinaryIO:
        raise NotImplementedError

    def read(self, norm: str) -> bytes:
        with self.open(norm) as f:
            return f.read()

    def sha256(self, norm: str) -> str:
        cache = self.__dict__.setdefault("_sha_cache", {})
        if norm not in cache:
            h = hashlib.sha256()
            with self.open(norm) as f:
                for chunk in iter(lambda: f.read(1 << 20), b""):
                    h.update(chunk)
            cache[norm] = h.hexdigest()
        return cache[norm]

    def has(self, norm: str) -> bool:
        raise NotImplementedError

    def content_hash(self) -> str:
        """Source-independent identity: the same skin as a folder or .osk hashes equally."""
        h = hashlib.sha256()
        for p in sorted(self.paths()):
            h.update(p.encode())
            h.update(b"\0")
            h.update(self.sha256(p).encode())
            h.update(b"\n")
        return h.hexdigest()


def _strip_root(files: dict[str, str]) -> dict[str, str]:
    """If skin.ini is not at the root but exactly one copy exists in a subfolder, re-root there."""
    if "skin.ini" in files:
        return files
    candidates = [p for p in files if p.rsplit("/", 1)[-1] == "skin.ini"]
    if len(candidates) != 1:
        return files
    prefix = candidates[0][: -len("skin.ini")]
    return {p[len(prefix):]: o for p, o in files.items() if p.startswith(prefix)}


class DirSource(SkinSource):
    kind = "folder"

    def __init__(self, root: Path):
        self.root = root
        self.name = root.name
        self.location = str(root)
        files: dict[str, str] = {}
        for dirpath, _dirs, names in os.walk(root):
            for n in names:
                rel = os.path.relpath(os.path.join(dirpath, n), root)
                files.setdefault(normalize(rel), rel)
        self._files = _strip_root(files)

    def paths(self) -> list[str]:
        return list(self._files)

    def has(self, norm: str) -> bool:
        return norm in self._files

    def original_path(self, norm: str) -> str:
        return self._files[norm]

    def open(self, norm: str) -> BinaryIO:
        return open(self.root / self._files[norm], "rb")


class OskSource(SkinSource):
    kind = "osk"

    def __init__(self, path: Path):
        self.path = path
        self.name = path.stem
        self.location = str(path)
        self._zip = zipfile.ZipFile(path)
        files: dict[str, str] = {}
        for info in self._zip.infolist():
            if not info.is_dir():
                files.setdefault(normalize(info.filename), info.filename)
        self._files = _strip_root(files)

    def paths(self) -> list[str]:
        return list(self._files)

    def has(self, norm: str) -> bool:
        return norm in self._files

    def original_path(self, norm: str) -> str:
        return self._files[norm]

    def open(self, norm: str) -> BinaryIO:
        return self._zip.open(self._files[norm])


def open_skin(path: Path) -> SkinSource:
    """Open exactly one skin: an extracted skin folder or a single .osk archive."""
    if path.is_dir():
        return DirSource(path)
    if path.is_file() and path.suffix.lower() in (".osk", ".zip"):
        return OskSource(path)
    raise ValueError(f"{path} is neither a skin folder nor an .osk archive")
