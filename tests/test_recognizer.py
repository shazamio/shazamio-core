"""Golden fingerprint: the URI `probe.flac` produces must not change silently.

A failure there means the signature the library emits for unchanged input changed.
That is either a bug or a deliberate algorithm change; in the second case
`probe.flac.uri` is rewritten by hand, in the same commit, with the reason in the
message. The audio itself comes from `tests/data/generate.sh`.

Only the `.flac` signature is pinned, and only on Linux. Two separate things put a
URI beyond what a golden file can hold.

For `.mp3` and `.ogg` it is the decoder. `symphonia` decodes them in `f32`, so the
same file yields a handful of peaks one quantisation step apart per target: against
goldens taken on x86_64 Linux, `[ogg]` failed on `windows-latest` and `[mp3]` and
`[ogg]` on `macos-latest`.
https://github.com/shazamio/shazamio-core/actions/runs/32988035458

For `.flac`, which decodes to identical samples everywhere, it is the resampler.
`rubato` builds its sinc table from `sin` and `cos`, so its last bits follow the
platform's libm. On `windows-latest` this file produced 162 peaks against 161, the
extra one sitting on the detection threshold in the 520 to 1450 Hz band and every
other peak identical.
https://github.com/shazamio/shazamio-core/actions/runs/33940712799

Neither is a decode error: the sample counts match on every platform, and that is
what the checks below assert, on every platform.
"""

import sys
from pathlib import Path
from typing import Final

import pytest

from shazamio_core import Recognizer

DATA_DIRECTORY: Final[Path] = Path(__file__).parent / "data"

AUDIO_FORMATS: Final[tuple[str, ...]] = ("mp3", "ogg", "opus", "flac")

GOLDEN_AUDIO_FORMAT: Final[str] = "flac"

# All three files encode the same 8-second source. `.samples` names a duration, not
#  a count: `src/fingerprinting/communication.rs` divides the sample count by the
#  sample rate, so the field is milliseconds. This is what guards the `.ogg` path now
#  that its URI is not pinned: an `.ogg` that decodes to nothing lands nowhere near it.
EXPECTED_DURATION_MS: Final[int] = 8000

# The resampler drops a few samples at each edge, and a lossy encoder pads the stream
#  it writes, so the decoded length lands beside the source length rather than on it:
#  7997 ms for `.flac` and `.ogg`, 8042 ms for `.mp3`.
DURATION_TOLERANCE_MS: Final[int] = 100


def _probe(audio_format: str) -> Path:
    return DATA_DIRECTORY / f"probe.{audio_format}"


@pytest.mark.skipif(sys.platform != "linux", reason="the golden URI is pinned on Linux")
async def test_the_flac_signature_matches_the_golden_uri(*, recognizer: Recognizer) -> None:
    golden_uri = (DATA_DIRECTORY / f"probe.{GOLDEN_AUDIO_FORMAT}.uri").read_text().strip()

    signature = await recognizer.recognize_path(_probe(GOLDEN_AUDIO_FORMAT))

    assert signature.signature.uri == golden_uri


@pytest.mark.parametrize("audio_format", AUDIO_FORMATS)
async def test_recognize_bytes_matches_recognize_path(
    audio_format: str,
    *,
    recognizer: Recognizer,
) -> None:
    audio = _probe(audio_format)

    from_bytes = await recognizer.recognize_bytes(audio.read_bytes())
    from_path = await recognizer.recognize_path(audio)

    assert from_bytes.signature.uri == from_path.signature.uri


@pytest.mark.parametrize("audio_format", AUDIO_FORMATS)
async def test_every_format_decodes_the_whole_file(
    audio_format: str,
    *,
    recognizer: Recognizer,
) -> None:
    signature = await recognizer.recognize_path(_probe(audio_format))

    assert abs(signature.signature.samples - EXPECTED_DURATION_MS) <= DURATION_TOLERANCE_MS


async def test_recognize_path_accepts_a_string_too(*, recognizer: Recognizer) -> None:
    # The tests above pass a `Path`. `recognize_path` extracts a Rust `PathBuf`
    #  through `os.fspath`, so both forms are accepted; before that it extracted a
    #  `String` and rejected a `Path` with `TypeError: 'PosixPath' object is not an
    #  instance of 'str'`, contradicting its own type stub.
    audio = _probe("mp3")

    from_string = await recognizer.recognize_path(str(audio))
    from_path = await recognizer.recognize_path(audio)

    assert from_string.signature.uri == from_path.signature.uri
