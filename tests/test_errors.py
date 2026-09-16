"""What the two entry points do with input they cannot use.

A refusal is a `SignatureError` on the `await`, or a `TypeError` at the call, and the
message names which input failed: `symphonia` describes the stream it was handed and
never where that stream came from, so a caller passing one of many files learns
nothing from `No such file or directory (os error 2)` on its own. The `README` says
what each refusal is; the asserts below hold the messages to it.
"""

import re
from pathlib import Path
from typing import Final

import pytest
from conftest import DATA_DIRECTORY

from shazamio_core import Recognizer, SignatureError

# The first 2 KiB of the file carry the FLAC header, so the probe recognises the
#  format and then runs out of stream. That is a different failure from a payload
#  nothing recognises at all, and both are asserted below.
_TRUNCATED_AUDIO: Final[bytes] = (DATA_DIRECTORY / "probe.flac").read_bytes()[:2048]

_UNREADABLE: Final[str] = "unsupported feature: no reader in this build recognises the stream"


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
        pytest.param(_TRUNCATED_AUDIO, "the byte payload: end of stream", id="truncated-audio"),
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
