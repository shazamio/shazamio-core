"""What the two entry points do with input they cannot use.

A refusal is a `SignatureError` on the `await`, or a `TypeError` at the call, and the
message names which input failed: `symphonia` describes the stream it was handed and
never where that stream came from, so a caller passing one of many files learns
nothing from `No such file or directory (os error 2)` on its own. The `README` says
what each refusal is; the asserts below hold the messages to it.
"""

import logging
import re
import struct
from pathlib import Path
from typing import Final

import pytest
from conftest import DATA_DIRECTORY

from shazamio_core import Recognizer, SignatureError

# The first 2 KiB end inside the FLAC metadata, before the first audio frame, so
#  the probe recognises the format and then runs out of stream. That is a
#  different failure from a payload nothing recognises at all, and both are
#  asserted below.
_CUT_BEFORE_AUDIO: Final[bytes] = (DATA_DIRECTORY / "probe.flac").read_bytes()[:2048]

_UNREADABLE: Final[str] = "unsupported feature: no reader in this build recognises the stream"

# A WAV header whose format tag, 0x0031, is GSM 6.10, which the WAV reader refuses
#  while opening the file.
_GSM_FORMAT: Final[bytes] = struct.pack("<HHIIHH", 0x0031, 1, 8000, 1625, 65, 0)
_GSM_IN_WAV: Final[bytes] = (
    b"RIFF"
    + struct.pack("<I", 4 + 8 + len(_GSM_FORMAT) + 8 + 650)
    + b"WAVEfmt "
    + struct.pack("<I", len(_GSM_FORMAT))
    + _GSM_FORMAT
    + b"data"
    + struct.pack("<I", 650)
    + bytes(650)
)


def _first_95_percent(name: str) -> bytes:
    payload = (DATA_DIRECTORY / name).read_bytes()
    return payload[: len(payload) * 95 // 100]


@pytest.mark.parametrize(
    "path",
    [
        pytest.param(DATA_DIRECTORY / "no-such-file.flac", id="missing-file"),
        pytest.param(DATA_DIRECTORY, id="directory"),
    ],
)
async def test_a_path_that_cannot_be_read_names_itself(
    path: Path,
    *,
    recognizer: Recognizer,
) -> None:
    # What follows the path is the operating system's wording, and it differs across
    #  the three platforms the suite runs on.
    with pytest.raises(SignatureError, match=f"^{re.escape(str(path))}: "):
        await recognizer.recognize_path(path)


@pytest.mark.parametrize(
    ("payload", "expected_message"),
    [
        pytest.param(b"", f"the byte payload: {_UNREADABLE}", id="empty"),
        pytest.param(b"not audio at all", f"the byte payload: {_UNREADABLE}", id="not-audio"),
        pytest.param(
            _CUT_BEFORE_AUDIO,
            "the byte payload: unexpected end of file",
            id="cut-before-audio",
        ),
        pytest.param(
            _first_95_percent("probe.m4a"),
            "the byte payload: unsupported feature: isomp4: missing moov atom",
            id="mp4-cut-before-moov",
        ),
        pytest.param(
            _first_95_percent("matroska.webm"),
            "the byte payload: unsupported feature: mkv: missing segment element",
            id="matroska-cut",
        ),
        pytest.param(
            _GSM_IN_WAV,
            "the byte payload: unsupported feature: wav: unsupported wave format",
            id="wav-holding-gsm",
        ),
    ],
)
async def test_a_payload_that_cannot_be_decoded_says_which_input_it_was(
    payload: bytes,
    expected_message: str,
    *,
    recognizer: Recognizer,
) -> None:
    with pytest.raises(SignatureError, match=f"^{re.escape(expected_message)}$"):
        await recognizer.recognize_bytes(payload)


async def test_a_codec_this_build_cannot_decode_is_refused(*, recognizer: Recognizer) -> None:
    path: Path = DATA_DIRECTORY / "ac3.mka"
    expected_message: str = (
        f"{path}: unsupported feature: the stream carries no track with a codec this build can decode"
    )

    with pytest.raises(SignatureError, match=f"^{re.escape(expected_message)}$"):
        await recognizer.recognize_path(path)


async def test_a_payload_nothing_recognises_is_not_logged_as_well(
    caplog: pytest.LogCaptureFixture,
    *,
    recognizer: Recognizer,
) -> None:
    # `symphonia` 0.6 logs every probe that finds no reader, so each refusal used to
    #  print `probe reached EOF at 16 bytes` on top of the raised error.
    with pytest.raises(SignatureError):
        await recognizer.recognize_bytes(b"not audio at all")

    assert [record for record in caplog.records if record.levelno >= logging.WARNING] == []
