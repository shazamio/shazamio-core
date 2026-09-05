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
