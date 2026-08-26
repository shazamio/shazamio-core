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

from pathlib import Path
from typing import Final

import pytest

from shazamio_core import Recognizer

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
    # Callers wrap this in `str()`: `recognize_path` extracts a Rust `String` and
    #  rejects a `Path` with `TypeError: 'PosixPath' object is not an instance of
    #  'str'`, even though `shazamio_core/shazamio_core.pyi:80` declares
    #  `Union[str, PathLike]`. It still returns a `Path` -- `recognize_bytes` needs
    #  `.read_bytes()`.
    return DATA_DIRECTORY / f"probe.{audio_format}"


async def test_the_flac_signature_matches_the_golden_uri(*, recognizer: Recognizer) -> None:
    golden_uri = (DATA_DIRECTORY / f"probe.{LOSSLESS_AUDIO_FORMAT}.uri").read_text().strip()

    signature = await recognizer.recognize_path(str(_probe(LOSSLESS_AUDIO_FORMAT)))

    assert signature.signature.uri == golden_uri


@pytest.mark.parametrize("audio_format", AUDIO_FORMATS)
async def test_recognize_bytes_matches_recognize_path(
    audio_format: str,
    *,
    recognizer: Recognizer,
) -> None:
    audio = _probe(audio_format)

    from_bytes = await recognizer.recognize_bytes(audio.read_bytes())
    from_path = await recognizer.recognize_path(str(audio))

    assert from_bytes.signature.uri == from_path.signature.uri


@pytest.mark.parametrize("audio_format", AUDIO_FORMATS)
async def test_every_format_decodes_the_whole_file(
    audio_format: str,
    *,
    recognizer: Recognizer,
) -> None:
    signature = await recognizer.recognize_path(str(_probe(audio_format)))

    assert signature.signature.samples == EXPECTED_DURATION_MS
