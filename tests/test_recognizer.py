"""Golden fingerprint: the URI `probe.flac` produces must not change silently.

A failure there means the signature the library emits for unchanged input changed.
That is either a bug or a deliberate algorithm change; in the second case
`probe.flac.uri` is rewritten by hand, in the same commit, with the reason in the
message. The audio itself comes from `tests/data/generate.sh`.

Only the `.flac` signature is pinned, because only FLAC decodes to identical
samples everywhere. `symphonia` decodes `.mp3` and `.ogg` in `f32`, and the same
file then yields a handful of peaks one quantisation step apart per target: against
goldens taken on x86_64 Linux, `[ogg]` failed on `windows-latest` and `[mp3]` and
`[ogg]` on `macos-latest`, while the sample counts matched everywhere.
https://github.com/shazamio/shazamio-core/actions/runs/32988035458
That is decoder arithmetic, not something a golden file can pin. The two lossy
formats keep the checks below, which hold on every platform.
"""

import os
import sys
from pathlib import Path
from typing import Final

import pytest

from shazamio_core import Recognizer, SignatureError

DATA_DIRECTORY: Final[Path] = Path(__file__).parent / "data"

AUDIO_FORMATS: Final[tuple[str, ...]] = ("mp3", "ogg", "flac")

# The one format whose signature is byte-identical across platforms -- see above.
LOSSLESS_AUDIO_FORMAT: Final[str] = "flac"

# All three files encode the same 8-second source. `.samples` names a duration, not
#  a count: `src/fingerprinting/communication.rs` divides the sample count by the
#  sample rate, so the field is milliseconds -- 8000 against 128013 real samples.
#  This is what guards the `.ogg` path now that its URI is not pinned: before
#  `NonEmptySpans` in `src/fingerprinting/algorithm.rs`, `symphonia` reported a
#  zero-length span for the first Vorbis packet, the resampler collapsed and `.ogg`
#  decoded to nothing.
EXPECTED_DURATION_MS: Final[int] = 8000


def _probe(audio_format: str) -> Path:
    return DATA_DIRECTORY / f"probe.{audio_format}"


async def test_the_flac_signature_matches_the_golden_uri(*, recognizer: Recognizer) -> None:
    golden_uri = (DATA_DIRECTORY / f"probe.{LOSSLESS_AUDIO_FORMAT}.uri").read_text().strip()

    signature = await recognizer.recognize_path(_probe(LOSSLESS_AUDIO_FORMAT))

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


async def test_recognize_path_accepts_a_string_too(*, recognizer: Recognizer) -> None:
    # The tests above pass a `Path`. `recognize_path` extracts a Rust `PathBuf`
    #  through `os.fspath`, so both forms are accepted; before that it extracted a
    #  `String` and rejected a `Path` with `TypeError: 'PosixPath' object is not an
    #  instance of 'str'`, contradicting its own type stub.
    audio = _probe("mp3")

    from_string = await recognizer.recognize_path(str(audio))
    from_path = await recognizer.recognize_path(audio)

    assert from_string.signature.uri == from_path.signature.uri


@pytest.mark.skipif(
    sys.platform != "linux",
    reason="only Linux lets a directory name be invalid UTF-8",
)
async def test_a_temp_directory_that_is_not_utf8_raises_instead_of_panicking(
    tmp_path: Path,
    *,
    recognizer: Recognizer,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    # The ffmpeg fallback builds its scratch paths under `TMPDIR`. Those paths used to
    #  be forced through `str`, so a directory Linux allows and UTF-8 does not aborted
    #  the tokio worker with `pyo3_async_runtimes.RustPanic: rust future panicked`,
    #  which no `except SignatureError` around the call can catch.
    broken_tmpdir = tmp_path / os.fsdecode(b"\xff")
    broken_tmpdir.mkdir()
    monkeypatch.setenv("TMPDIR", str(broken_tmpdir))

    # Not decodable by `rodio`, so the call reaches the ffmpeg fallback.
    with pytest.raises(SignatureError):
        await recognizer.recognize_bytes(b"\x00" * 4096)
