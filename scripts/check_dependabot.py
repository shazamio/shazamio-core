#!/usr/bin/env python3
"""Check every tracked manifest sits in a directory its Dependabot entry names."""

import subprocess
import sys
from fnmatch import fnmatchcase
from pathlib import Path, PurePosixPath
from typing import Final

import yaml

_CONFIG: Final[Path] = Path(".github/dependabot.yml")

# The file name each ecosystem reads in a directory. Dependabot's `docker` pattern
#  is `/dockerfile|containerfile/i`, so `regenerate.Dockerfile` counts too; the tree
#  has no `Containerfile`.
#  https://github.com/dependabot/dependabot-core/blob/c96ebb6ae58f869c275e4e1d220d3bc00cd28be9/docker/lib/dependabot/docker/file_fetcher.rb#L12
_MANIFESTS: Final[dict[str, str]] = {
    "cargo": "Cargo.toml",
    "uv": "pyproject.toml",
    "pre-commit": ".pre-commit-config.yaml",
    "docker": "*[dD]ockerfile*",
    "github-actions": "action.y*ml",
}


def _tracked() -> list[str]:
    listed = subprocess.run(
        ["git", "ls-files"],
        capture_output=True,
        check=True,
        text=True,
    )

    return listed.stdout.splitlines()


def _covered() -> dict[str, list[str]]:
    """Map each configured ecosystem to the directories its entries name."""
    covered: dict[str, list[str]] = {}

    for entry in yaml.safe_load(_CONFIG.read_text(encoding="utf-8"))["updates"]:
        named = entry.get("directories", [entry.get("directory")])
        covered.setdefault(entry["package-ecosystem"], []).extend(named)

    return covered


def main() -> int:
    covered = _covered()
    problems: list[str] = []

    for path in _tracked():
        manifest = PurePosixPath(path)
        directory = str(PurePosixPath("/") / manifest.parent)

        # An ecosystem with no entry at all is a decision, not a gap this can see.
        for ecosystem in covered.keys() & _MANIFESTS.keys():
            if not fnmatchcase(manifest.name, _MANIFESTS[ecosystem]):
                continue

            if any(fnmatchcase(directory, name.rstrip("/") or "/") for name in covered[ecosystem]):
                continue

            problems.append(f"{_CONFIG}: no `{ecosystem}` entry names `{directory}`, so `{path}` is never bumped")

    for problem in problems:
        print(problem, file=sys.stderr)

    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
