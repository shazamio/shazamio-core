#!/usr/bin/env python3
"""Check the aggregate gate job waits on every job that gates a merge."""

import sys
from pathlib import Path
from typing import Final

import yaml

_WORKFLOW: Final[Path] = Path(".github/workflows/ci.yaml")

_GATE: Final[str] = "gate"

# `release` publishes rather than gates: it runs on a tag alone and keeps its own
#  `needs` list, for the reason stated beside it.
_NOT_A_GATE: Final[frozenset[str]] = frozenset({"release"})


def main() -> int:
    jobs = yaml.safe_load(_WORKFLOW.read_text(encoding="utf-8"))["jobs"]

    waited_on = frozenset(jobs[_GATE]["needs"])
    should_wait_on: frozenset[str] = frozenset(jobs) - _NOT_A_GATE - {_GATE}

    missing = sorted(should_wait_on - waited_on)
    unexpected = sorted(waited_on - should_wait_on)

    if missing:
        print(f"{_WORKFLOW}: `{_GATE}` does not wait on {', '.join(missing)}", file=sys.stderr)

    if unexpected:
        print(f"{_WORKFLOW}: `{_GATE}` waits on {', '.join(unexpected)}, which gates nothing", file=sys.stderr)

    return 1 if missing or unexpected else 0


if __name__ == "__main__":
    sys.exit(main())
