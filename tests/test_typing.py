"""What a caller may do with the public API, checked by a type checker and at runtime.

`mypy.stubtest` compares declarations against the extension and never reads a call
site, so nothing else here notices the stub promising a coroutine the runtime does
not build, or a writable attribute it refuses to assign. `just typecheck` runs
`mypy --strict` over this file, where every ignore below goes unused the moment the
stub goes back to what it said before and `--warn-unused-ignores` fails on it.
"""

import asyncio
from pathlib import Path
from typing import Final

import pytest
from conftest import DATA_DIRECTORY

from shazamio_core import Geolocation, Recognizer, SearchParams, Signature, SignatureSong

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


def test_the_geolocation_fields_cannot_be_assigned() -> None:
    geolocation = Geolocation(
        altitude=0,
        latitude=0,
        longitude=0,
    )

    with pytest.raises(AttributeError):
        geolocation.altitude = 1  # type: ignore[misc]

    with pytest.raises(AttributeError):
        geolocation.latitude = 1  # type: ignore[misc]

    with pytest.raises(AttributeError):
        geolocation.longitude = 1  # type: ignore[misc]


def test_the_signature_song_fields_cannot_be_assigned() -> None:
    song = SignatureSong(
        samples=0,
        timestamp=0,
        uri="",
    )

    with pytest.raises(AttributeError):
        song.samples = 1  # type: ignore[misc]

    with pytest.raises(AttributeError):
        song.timestamp = 1  # type: ignore[misc]

    with pytest.raises(AttributeError):
        song.uri = "other"  # type: ignore[misc]


def test_the_signature_fields_cannot_be_assigned() -> None:
    song = SignatureSong(
        samples=0,
        timestamp=0,
        uri="",
    )
    geolocation = Geolocation(
        altitude=0,
        latitude=0,
        longitude=0,
    )
    signature = Signature(
        geolocation=geolocation,
        signature=song,
        timestamp=0,
        timezone="",
    )

    with pytest.raises(AttributeError):
        signature.geolocation = geolocation  # type: ignore[misc]

    with pytest.raises(AttributeError):
        signature.signature = song  # type: ignore[misc]

    with pytest.raises(AttributeError):
        signature.timestamp = 1  # type: ignore[misc]

    with pytest.raises(AttributeError):
        signature.timezone = "other"  # type: ignore[misc]


def test_the_segment_duration_stays_writable() -> None:
    """The one pair with a setter, so neither assignment below carries an ignore."""
    recognizer = Recognizer()
    search_parameters = SearchParams()

    recognizer.segment_duration_seconds = 5
    search_parameters.segment_duration_seconds = 5

    assert recognizer.segment_duration_seconds == 5
    assert search_parameters.segment_duration_seconds == 5
