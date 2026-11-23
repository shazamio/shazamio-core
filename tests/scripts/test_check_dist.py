import sys
import zipfile
from pathlib import Path
from typing import Final

import pytest

from check_dist import _set_problems, _tag_problems, main

_PLATFORM: Final[str] = "manylinux_2_28_x86_64"


def _wheel(tags: str) -> str:
    return f"shazamio_core-2.1.0-{tags}-{_PLATFORM}.whl"


@pytest.mark.parametrize(
    ("tags", "expected"),
    [
        pytest.param("cp311-abi3", [], id="abi3-at-the-floor"),
        pytest.param("cp314-cp314t", [], id="free-threaded-before-abi3t"),
        pytest.param("cp315-abi3.abi3t", [], id="abi3t-at-its-floor"),
        pytest.param("pp311-pypy311_pp73", [], id="pypy"),
        pytest.param(
            "cp314-abi3.abi3t",
            ["is tagged `cp314-abi3.abi3t`, older than the 3.15 that `abi3t` starts at"],
            id="abi3t-below-its-floor",
        ),
        pytest.param(
            "cp315-cp315t",
            ["is tagged `cp315-cp315t`, a free-threaded build the `abi3t` wheel covers"],
            id="free-threaded-the-abi3t-wheel-covers",
        ),
        pytest.param(
            "cp311-cp311",
            ["is tagged `cp311-cp311`, so it is not a limited API wheel"],
            id="version-specific-with-a-gil",
        ),
    ],
)
def test_a_wheel_is_judged_by_its_tags(*, tags: str, expected: list[str]) -> None:
    assert _tag_problems(_wheel(tags), requires_python=">=3.11") == expected


@pytest.mark.parametrize(
    ("tags", "expected"),
    [
        pytest.param(["cp311-abi3", "cp314-cp314t", "cp315-abi3.abi3t", "pp311-pypy311_pp73"], [], id="every-kind"),
        pytest.param([], [], id="no-wheel"),
        pytest.param(["pp311-pypy311_pp73"], [], id="pypy-only"),
        pytest.param(
            ["cp311-abi3", "pp311-pypy311_pp73"],
            [
                "no `abi3.abi3t` wheel, so no free-threaded CPython from 3.15 gets one",
                "no version-specific free-threaded wheel, so no free-threaded CPython before 3.15 gets one",
            ],
            id="abi3-only",
        ),
        pytest.param(
            ["cp314-cp314t", "cp315-abi3.abi3t"],
            ["no `abi3` wheel, so no CPython with the GIL gets one"],
            id="free-threaded-only",
        ),
    ],
)
def test_a_directory_needs_every_kind_of_cpython_wheel(*, tags: list[str], expected: list[str]) -> None:
    assert _set_problems([_wheel(tag) for tag in tags]) == expected


def test_main_fails_a_directory_missing_a_kind_of_wheel(
    *,
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
) -> None:
    zipfile.ZipFile(tmp_path / _wheel("cp311-abi3"), "w").close()
    monkeypatch.setattr(sys, "argv", ["check_dist.py", str(tmp_path)])

    assert main() == 1
    assert f"{tmp_path}: no `abi3.abi3t` wheel, so no free-threaded CPython from 3.15 gets one" in capsys.readouterr().err
