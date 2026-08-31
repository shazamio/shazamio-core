"""Fixtures shared by the fingerprint tests."""

import pytest

from shazamio_core import Recognizer


@pytest.fixture
def recognizer() -> Recognizer:
    """The recognizer every fingerprint test runs against."""
    # Left on the default 10-second segment against 8-second probes, so the whole
    #  file is analysed and `EXPECTED_DURATION_MS` stays a constant. A duration below 8
    #  would centre-crop the audio instead and change every golden `.uri`.
    return Recognizer()


@pytest.fixture(autouse=True)
def without_ffmpeg(monkeypatch: pytest.MonkeyPatch) -> None:
    """Empty `PATH` so nothing can resolve `ffmpeg`."""
    # `decode_with_ffmpeg` shells out whenever `rodio` refuses a file, so on a machine
    # that has `ffmpeg` installed the golden values could come from either decoder and
    # the tests would prove nothing about `rodio`. An empty `PATH` leaves only the one
    # path under test. It cannot be faked with a stub binary portably -- Windows needs
    # a real executable -- and emptying `PATH` needs no file at all.
    monkeypatch.setenv("PATH", "")
