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

from shazamio_core import Recognizer, SearchParams

DATA_DIRECTORY: Final[Path] = Path(__file__).parent / "data"

AUDIO_FORMATS: Final[tuple[str, ...]] = ("mp3", "ogg", "opus", "flac")

GOLDEN_AUDIO_FORMAT: Final[str] = "flac"

# Every file encodes the same 8-second source and decodes to exactly that, because
#  the reader trims the padding a lossy encoder writes. `.samples` is a duration in
#  milliseconds, not a count: `src/fingerprinting/communication.rs` does the division.
EXPECTED_DURATION_MS: Final[int] = 8000


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

    assert signature.signature.samples == EXPECTED_DURATION_MS


@pytest.mark.parametrize(
    ("file_name", "expected_duration_ms"),
    [
        pytest.param("matroska.webm", 8013, id="opus-in-matroska"),
        pytest.param("surround.opus", 8000, id="six-channel-opus"),
    ],
)
async def test_a_wider_opus_stream_decodes(
    file_name: str,
    expected_duration_ms: int,
    *,
    recognizer: Recognizer,
) -> None:
    # Neither file is in the matrix above, and both were refused until the decoder
    #  started reading `OpusHead`: Matroska declares no channel count, and six channels
    #  need the multistream API. Its end padding is dropped, not applied, hence 8013.
    signature = await recognizer.recognize_path(DATA_DIRECTORY / file_name)

    assert signature.signature.samples == expected_duration_ms


@pytest.mark.parametrize("segment_duration_seconds", [268_436, 4_294_967_295])
async def test_a_segment_longer_than_the_file_analyses_it_whole(
    segment_duration_seconds: int,
    *,
    recognizer: Recognizer,
) -> None:
    # The window was computed in a type too narrow to hold it, so 268436 seconds
    #  selected 544 ms of this 8-second file and the largest accepted value nothing
    #  at all, both reporting success.
    signature = await recognizer.recognize_path(
        _probe(GOLDEN_AUDIO_FORMAT),
        SearchParams(segment_duration_seconds),
    )

    assert signature.signature.samples == EXPECTED_DURATION_MS


async def test_recognize_path_accepts_a_string_too(*, recognizer: Recognizer) -> None:
    # `recognize_path` extracts a Rust `PathBuf` through `os.fspath`, so a `str` and
    #  a `Path` both work. It used to extract a `String` and reject a `Path` with
    #  `TypeError: 'PosixPath' object is not an instance of 'str'`, against its stub.
    audio = _probe("mp3")

    from_string = await recognizer.recognize_path(str(audio))
    from_path = await recognizer.recognize_path(audio)

    assert from_string.signature.uri == from_path.signature.uri
