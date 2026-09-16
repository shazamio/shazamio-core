"""What a caller may do with what the entry points return.

`mypy.stubtest` compares declarations against the extension and never reads a call
site, so nothing else here notices the stub promising a coroutine the runtime does
not build. `just typecheck` runs `mypy --strict` over this file, where the ignore
below goes unused the moment the stub says `async def` again and
`--warn-unused-ignores` fails on it.
"""

import asyncio
from pathlib import Path
from typing import Final

import pytest
from conftest import DATA_DIRECTORY

from shazamio_core import Recognizer

_PROBE: Final[Path] = DATA_DIRECTORY / "probe.flac"


async def test_awaiting_the_call_gives_a_signature(*, recognizer: Recognizer) -> None:
    signature = await recognizer.recognize_path(_PROBE)

    assert signature.signature.uri


async def test_both_entry_points_schedule_without_being_awaited(*, recognizer: Recognizer) -> None:
    pending = asyncio.ensure_future(recognizer.recognize_path(_PROBE))
    gathered = asyncio.gather(recognizer.recognize_bytes(_PROBE.read_bytes()))

    assert (await pending).signature.uri
    assert len(await gathered) == 1


async def test_scheduling_the_call_as_a_task_fails(*, recognizer: Recognizer) -> None:
    """A `Future` is not a coroutine, and `create_task` takes a coroutine alone."""
    pending = recognizer.recognize_path(_PROBE)

    with pytest.raises(TypeError):
        asyncio.create_task(pending)  # type: ignore[arg-type]

    await pending
