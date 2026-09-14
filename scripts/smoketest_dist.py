#!/usr/bin/env python3
"""Install the built wheels outside the checkout and exercise what they carry."""

import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import Final

# `python -c` puts the working directory first on `sys.path`, so the probe runs
#  from a temporary directory rather than the checkout. It checks what it loaded
#  regardless: a wheel carrying no extension imports out of the source tree and
#  passes any check that only prints the path.
_PROBE: Final[str] = """
import sys
from pathlib import Path

import shazamio_core

loaded = Path(shazamio_core.shazamio_core.__file__).resolve()

if loaded.is_relative_to(Path(sys.argv[1])):
    raise SystemExit(f"imported the checkout rather than the wheel: {loaded}")

# Loading the extension proves it links; calling into it proves it runs.
shazamio_core.Geolocation(altitude=300, latitude=45, longitude=2)

print(loaded)
"""


def main() -> int:
    if len(sys.argv) != 2:
        print(f"usage: {sys.argv[0]} <directory of built wheels>", file=sys.stderr)
        return 2

    directory = Path(sys.argv[1])
    wheels = sorted(directory.glob("*.whl"))

    if not wheels:
        print(f"{directory}: no wheel to install", file=sys.stderr)
        return 1

    checkout = Path(__file__).resolve().parent.parent

    # `uv run --with` layers onto whatever virtualenv is active rather than
    #  replacing it, so from a checkout whose `.venv` holds the editable install
    #  the probe imports that instead of the wheel. A CI runner has no such
    #  environment, so the check would pass there and mean nothing here.
    environment = dict(os.environ)
    environment.pop("VIRTUAL_ENV", None)

    with tempfile.TemporaryDirectory() as outside:
        for wheel in wheels:
            shutil.copy(wheel, outside)

        # Resolving by name is what fails on a wheel carrying the wrong platform
        #  tags, where naming the file would install it regardless. `--no-index`
        #  keeps the index from satisfying that name behind our back.
        command = [
            "uv", "run", "--no-project", "--no-index", "--find-links", ".",
            "--with", "shazamio_core",
            "python", "-c", _PROBE, str(checkout),
        ]

        completed = subprocess.run(
            command,
            cwd=outside,
            env=environment,
            check=False,
        )

    return completed.returncode


if __name__ == "__main__":
    sys.exit(main())
