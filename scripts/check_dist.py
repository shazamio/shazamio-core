#!/usr/bin/env python3
"""Check that a built wheel or source distribution carries what it should."""

import re
import sys
import tarfile
import zipfile
from dataclasses import dataclass
from fnmatch import fnmatch
from pathlib import Path
from typing import Final

_FLOOR: Final[re.Pattern[str]] = re.compile(r">=3\.(\d+)")


@dataclass(frozen=True, slots=True)
class Rules:
    """What an artifact of one kind must carry, must not carry, and reads metadata from."""

    required: tuple[tuple[str, ...], ...]
    forbidden: tuple[str, ...]
    metadata_entry: str


@dataclass(frozen=True, slots=True)
class Artifact:
    entries: tuple[str, ...]
    metadata: str


# The three fields the two manifests argue over: `Summary` reaches the metadata only
#  through `dynamic`, `License-Expression` only from a literal `license`, and
#  `Requires-Python` is what `pip` reads before it installs anything. Each is empty
#  rather than wrong when the wiring breaks, which no build reports.
_REQUIRED_METADATA: Final[tuple[str, ...]] = ("Summary", "License-Expression", "Requires-Python")

# A group is one requirement: at least one of its patterns has to match. The
#  extension is a group because its suffix is `.pyd` on Windows and `.so` elsewhere,
#  and the PyPy wheels name it after the interpreter rather than `abi3`. Nothing is
#  forbidden: `[tool.maturin]` names no `include`, so what a wheel carries is
#  `maturin`'s decision alone.
_WHEEL: Final[Rules] = Rules(
    required=(
        ("shazamio_core/__init__.py",),
        ("shazamio_core/py.typed",),
        ("shazamio_core/shazamio_core.pyi",),
        ("shazamio_core/*.so", "shazamio_core/*.pyd"),
        ("*.dist-info/licenses/LICENSE",),
        ("*.dist-info/licenses/THIRD-PARTY-NOTICES.md",),
    ),
    forbidden=(),
    metadata_entry="*.dist-info/METADATA",
)

# Only what can go missing with nothing saying so. `Cargo.toml`, `Cargo.lock`,
#  `pyproject.toml` and `PKG-INFO` are added by the build whatever `include` says,
#  and a missing `README.md` fails it outright, so none of them can reach here.
#  The forbidden half makes a widened pattern in `include` fail rather than ship: a
#  built extension is named because `include` overrides `.gitignore`.
_SDIST: Final[Rules] = Rules(
    required=(
        ("LICENSE",),
        ("THIRD-PARTY-NOTICES.md",),
        ("justfile",),
        ("licenses/about.toml",),
        ("licenses/notices.hbs",),
        ("scripts/check_dist.py",),
        ("scripts/smoketest_dist.py",),
        ("shazamio_core/__init__.py",),
        ("shazamio_core/py.typed",),
        ("shazamio_core/shazamio_core.pyi",),
        ("tests/*.py",),
    ),
    forbidden=(
        ".github/*",
        ".gitignore",
        ".pre-commit-config.yaml",
        "docker/*",
        "shazamio_core/*.pyd",
        "shazamio_core/*.so",
        "uv.lock",
    ),
    metadata_entry="PKG-INFO",
)


# A CPython wheel is built against the `pyo3/abi3-py*` floor, so `maturin` tags it
#  `cp3XY-abi3` and it claims to load on every CPython from that floor up. Drop that
#  feature and the build silently goes back to one wheel per interpreter version,
#  with identical entries and metadata. That floor is set in `Cargo.toml` and the
#  `Requires-Python` one in `pyproject.toml`, so they can drift apart. PyPy has no
#  stable ABI and is tagged after its interpreter, argued in `docker/build-wheels.sh`.
def _tag_problems(name: str, *, requires_python: str | None) -> list[str]:
    """Report what is wrong with a wheel's tags, read from its name, given its `Requires-Python`."""
    python_tag, abi_tag = name.removesuffix(".whl").split("-")[-3:-1]

    if not python_tag.startswith("cp"):
        return []

    if abi_tag != "abi3":
        return [f"is tagged `{python_tag}-{abi_tag}`, so it is not a limited API wheel"]

    # A missing field is reported with the rest of the metadata.
    if requires_python is None:
        return []

    floor = _FLOOR.fullmatch(requires_python)

    if floor is None:
        return [f"declares `Requires-Python: {requires_python}`, which is not the `>=3.N` form compared with the tag"]

    if python_tag != f"cp3{floor[1]}":
        return [f"is tagged `{python_tag}` but declares `Requires-Python: {requires_python}`"]

    return []


def _matches(entries: tuple[str, ...], *, pattern: str) -> list[str]:
    return [entry for entry in entries if fnmatch(entry, pattern)]


def _read_wheel(path: Path) -> Artifact:
    with zipfile.ZipFile(path) as archive:
        entries = tuple(sorted(archive.namelist()))
        found = _matches(entries, pattern=_WHEEL.metadata_entry)
        metadata = archive.read(found[0]).decode() if len(found) == 1 else ""

    return Artifact(
        entries=entries,
        metadata=metadata,
    )


def _read_sdist(path: Path) -> Artifact:
    with tarfile.open(path) as archive:
        members = archive.getnames()

        # Every path carries a `<name>-<version>/` prefix that says nothing about what
        #  was shipped, so the rules above are written without it.
        prefix = members[0].split("/", 1)[0]
        entries = tuple(sorted(member.split("/", 1)[1] for member in members if "/" in member))

        found = _matches(entries, pattern=_SDIST.metadata_entry)
        extracted = archive.extractfile(f"{prefix}/{found[0]}") if len(found) == 1 else None
        metadata = extracted.read().decode() if extracted is not None else ""

    return Artifact(
        entries=entries,
        metadata=metadata,
    )


def _headers(metadata: str) -> dict[str, str]:
    headers: dict[str, str] = {}

    # Only the headers: the body after the first blank line is the long description,
    #  and a line of it can look exactly like one.
    for line in metadata.splitlines():
        if not line:
            break

        name, separator, value = line.partition(":")

        if separator and value.strip():
            headers[name] = value.strip()

    return headers


def _problems(artifact: Artifact, *, rules: Rules) -> list[str]:
    found: list[str] = []

    for group in rules.required:
        if not any(_matches(artifact.entries, pattern=pattern) for pattern in group):
            found.append(f"missing {' or '.join(group)}")

    for pattern in rules.forbidden:
        for entry in _matches(artifact.entries, pattern=pattern):
            matched = "" if entry == pattern else f", matched by `{pattern}`"
            found.append(f"ships {entry}{matched}")

    declared = _headers(artifact.metadata)

    for field in _REQUIRED_METADATA:
        if field not in declared:
            found.append(f"declares no `{field}` in {rules.metadata_entry}")

    return found


def _check(path: Path, *, artifact: Artifact, rules: Rules, name_problems: list[str]) -> bool:
    """Report what is wrong with one artifact, and answer whether anything was."""
    found: list[str] = name_problems + _problems(artifact, rules=rules)

    for problem in found:
        print(f"{path.name}: {problem}", file=sys.stderr)

    if not found:
        print(f"{path.name}: {len(artifact.entries)} entries, all checks passed")

    return bool(found)


def main() -> int:
    if len(sys.argv) != 2:
        print(f"usage: {sys.argv[0]} <directory of built artifacts>", file=sys.stderr)
        return 2

    directory = Path(sys.argv[1])
    wheels = sorted(directory.glob("*.whl"))
    sdists = sorted(directory.glob("*.tar.gz"))

    if not wheels and not sdists:
        print(f"{directory}: no wheel and no source distribution to check", file=sys.stderr)
        return 1

    failed: bool = False

    for path in wheels:
        artifact = _read_wheel(path)
        failed |= _check(
            path,
            artifact=artifact,
            rules=_WHEEL,
            name_problems=_tag_problems(
                path.name,
                requires_python=_headers(artifact.metadata).get("Requires-Python"),
            ),
        )

    for path in sdists:
        failed |= _check(
            path,
            artifact=_read_sdist(path),
            rules=_SDIST,
            name_problems=[],
        )

    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
