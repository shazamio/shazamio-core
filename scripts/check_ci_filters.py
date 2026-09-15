#!/usr/bin/env python3
"""Check every input a CI check reads is in the filter set that gates it."""

import re
import subprocess
import sys
import tomllib
from pathlib import Path
from typing import Final

import yaml

# A filter is a list of patterns, and an alias to another filter nests that filter's
#  list inside it, at any depth.
type PatternsType = str | list[PatternsType]

_FILTERS: Final[Path] = Path(".github/path-filters.yaml")

_WORKFLOW: Final[Path] = Path(".github/workflows/ci.yaml")

_HOOKS: Final[Path] = Path(".pre-commit-config.yaml")

_PYPROJECT: Final[Path] = Path("pyproject.toml")

# The key of `_FILTERS` holding what decides for every set, rather than what any
#  one check reads.
_DECIDER: Final[str] = "harness"

# Tracked files no check reads. `.gitignore` and `.pre-commit-config.yaml` configure
#  tools CI never runs, and nothing validates `dependabot.yml` at all.
_UNGATED: Final[frozenset[str]] = frozenset(
    {
        ".github/dependabot.yml",
        ".gitignore",
        ".pre-commit-config.yaml",
    }
)

# Which local hook restates which filter. A hook with a `files:` pattern missing
#  from here fails the check: a new one has to say what it mirrors.
_MIRRORS: Final[dict[str, str]] = {
    "lint-rust": "rust",
    "test-rust": "rust",
    "msrv": "rust",
    "test-python": "python",
    "typecheck": "python",
    "licenses": "licenses",
    "lint-harness": "harness_lint",
}

# A hook and a filter disagreeing over a whole directory is the ordinary case, and
#  one line per file buries every other problem under it.
_EXAMPLES: Final[int] = 3


def _tracked() -> frozenset[str]:
    listed = subprocess.run(
        ["git", "ls-files"],
        capture_output=True,
        check=True,
        text=True,
    )

    return frozenset(listed.stdout.splitlines())


def _patterns(value: PatternsType) -> list[str]:
    """Flatten one filter into its patterns; an alias to another set nests a list."""
    if isinstance(value, str):
        return [value]

    return [pattern for entry in value for pattern in _patterns(entry)]


# The action matches with `picomatch` and `{dot: true}`, so a leading dot is an
#  ordinary character and prefix matching is enough for the shapes below.
#  https://github.com/dorny/paths-filter/blob/ceb8a2b8f2d89434be7ff52d3de7ec3738c5cc9d/src/filter.ts#L14-L17
def _selects(pattern: str, *, path: str) -> bool:
    """Answer whether one `dorny/paths-filter` pattern selects one path."""
    if pattern.endswith("/**"):
        return path.startswith(pattern.removesuffix("**"))

    if pattern.startswith("**/*."):
        return path.endswith(pattern.removeprefix("**/*"))

    # A fourth shape would be answered wrongly rather than not at all, which is the
    #  one failure a check like this must never have.
    if "*" in pattern:
        msg = f"{_FILTERS}: `{pattern}` is a shape this check cannot read"
        raise ValueError(msg)

    return path == pattern


def _selected(value: PatternsType, *, tracked: frozenset[str]) -> frozenset[str]:
    patterns = _patterns(value)

    return frozenset(path for path in tracked if any(_selects(one, path=path) for one in patterns))


def _gating() -> frozenset[str]:
    """The filters the workflow reads; every other key of the file is an anchor."""
    outputs = yaml.safe_load(_WORKFLOW.read_text(encoding="utf-8"))["jobs"]["changes"]["outputs"]

    return frozenset(
        name
        for value in outputs.values()
        for name in re.findall(r"steps\.filter\.outputs\.(\w+)", value)
    )


def _ungated_inputs(*, sets: dict[str, frozenset[str]], tracked: frozenset[str]) -> list[str]:
    gated = frozenset().union(*sets.values())
    found = sorted(tracked - gated - _UNGATED)

    return [f"{_FILTERS}: no filter names `{path}`, so no check reports on it" for path in found]


def _ungated_packaging(*, sets: dict[str, frozenset[str]], tracked: frozenset[str]) -> list[str]:
    """Check the files `pyproject.toml` names as packaged are gated on the dist jobs."""
    # Only what a manifest names: `include` in `Cargo.toml` decides the rest, and its
    #  entries are archive paths that nothing here maps back to a tracked file.
    project = tomllib.loads(_PYPROJECT.read_text(encoding="utf-8"))["project"]
    packaged = (project["readme"], *project["license-files"])

    found: list[str] = []

    for path in packaged:
        if path not in tracked:
            found.append(f"{_PYPROJECT}: packages `{path}`, which is not a tracked file")
            continue

        if path not in sets["dist"]:
            found.append(f"{_FILTERS}: `dist` does not name `{path}`, which the wheel ships")

    return found


def _summarize(paths: frozenset[str], *, hook: str, reason: str) -> list[str]:
    if not paths:
        return []

    shown = ", ".join(sorted(paths)[:_EXAMPLES])
    rest = "" if len(paths) <= _EXAMPLES else ", ..."

    return [f"{_HOOKS}: `{hook}` {reason}, {len(paths)} of them: {shown}{rest}"]


def _hook_drift(
    *,
    sets: dict[str, frozenset[str]],
    tracked: frozenset[str],
    ignored: frozenset[str],
) -> list[str]:
    repos = yaml.safe_load(_HOOKS.read_text(encoding="utf-8"))["repos"]
    local = next(repo for repo in repos if repo["repo"] == "local")

    found: list[str] = []

    for hook in local["hooks"]:
        identifier = hook["id"]

        if "files" not in hook:
            continue

        if identifier not in _MIRRORS:
            found.append(f"{_HOOKS}: `{identifier}` restates no filter of {_FILTERS}")
            continue

        name = _MIRRORS[identifier]
        pattern = re.compile(hook["files"])

        matched: frozenset[str] = frozenset(path for path in tracked if pattern.search(path))

        selected: frozenset[str] = matched - ignored
        expected: frozenset[str] = sets[name] - ignored

        found += _summarize(
            selected - expected,
            hook=identifier,
            reason=f"runs on files `{name}` does not name",
        )
        found += _summarize(
            expected - selected,
            hook=identifier,
            reason=f"skips files `{name}` names",
        )

    return found


def main() -> int:
    tracked = _tracked()
    filters = yaml.safe_load(_FILTERS.read_text(encoding="utf-8"))

    sets = {name: _selected(filters[name], tracked=tracked) for name in _gating()}

    # Each config names whatever decides for it, and the two lists differ on purpose:
    #  the workflow carries `.github/**` and the hooks carry themselves. Dropped from
    #  both sides so the comparison is about what each check reads.
    ignored: frozenset[str] = _selected(filters[_DECIDER], tracked=tracked) | _UNGATED

    problems = [
        *_ungated_inputs(
            sets=sets,
            tracked=tracked,
        ),
        *_ungated_packaging(
            sets=sets,
            tracked=tracked,
        ),
        *_hook_drift(
            sets=sets,
            tracked=tracked,
            ignored=ignored,
        ),
    ]

    for problem in problems:
        print(problem, file=sys.stderr)

    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
