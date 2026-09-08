"""Peak memory: fingerprinting must not scale with the length of the recording.

The pipeline decodes, mixes down and resamples one packet at a time, so the only
thing that grows with the source is the 16 kHz mono result the fingerprint reads.
Held whole instead, a 30-minute stereo FLAC peaked at 1122188 KiB against 82200 KiB
for the same file today, both measured with `/usr/bin/time -v`.

The recording is written here rather than committed: five minutes of stereo is
53 MB, and a file short enough to commit makes the difference invisible.
"""

import math
import subprocess
import sys
import wave
from array import array
from pathlib import Path
from typing import Final

import pytest

_RECORDING_SECONDS: Final[int] = 300

_SOURCE_RATE_HZ: Final[int] = 44100
_SOURCE_CHANNEL_COUNT: Final[int] = 2

_TONE_HZ: Final[int] = 440
_TONE_AMPLITUDE: Final[int] = 12000

_BYTES_PER_SAMPLE: Final[int] = 2
_FINGERPRINT_RATE_HZ: Final[int] = 16000

# What the pipeline may hold beyond an interpreter that only imported the extension:
#  four times the 16 kHz mono stream of the recording, which leaves room for the segment
#  copy and the allocator. Buffering the source instead costs seven times this bound.
_PEAK_ALLOWANCE_KIB: Final[int] = (
    4 * _RECORDING_SECONDS * _FINGERPRINT_RATE_HZ * _BYTES_PER_SAMPLE // 1024
)

# Each run reports its own `VmHWM`, the peak of the mapping `execve` gave it. Measured
#  any other way it carries the caller's peak too: with 128 MiB held here, both runs
#  below returned the same `ru_maxrss` of 145964 KiB and the difference was 0.
#  https://github.com/torvalds/linux/blob/adc218676eef25575469234709c2d87185ca223a/fs/proc/task_mmu.c#L53
_PEAK_REPORTER: Final[str] = """
import asyncio
import sys

from shazamio_core import Recognizer


async def main() -> None:
    if len(sys.argv) > 1:
        await Recognizer().recognize_path(sys.argv[1])


asyncio.run(main())

with open("/proc/self/status") as status:
    print(next(line.split()[1] for line in status if line.startswith("VmHWM:")))
"""


def _write_recording(path: Path) -> None:
    one_second = array(
        "h",
        [
            int(_TONE_AMPLITUDE * math.sin(2 * math.pi * _TONE_HZ * frame / _SOURCE_RATE_HZ))
            for frame in range(_SOURCE_RATE_HZ)
            for _ in range(_SOURCE_CHANNEL_COUNT)
        ],
    )

    with wave.open(str(path), "wb") as recording:
        recording.setnchannels(_SOURCE_CHANNEL_COUNT)
        recording.setsampwidth(_BYTES_PER_SAMPLE)
        recording.setframerate(_SOURCE_RATE_HZ)

        for _ in range(_RECORDING_SECONDS):
            recording.writeframes(one_second.tobytes())


def _peak_kibibytes(recording: Path | None = None) -> int:
    arguments = [] if recording is None else [str(recording)]

    completed = subprocess.run(
        [sys.executable, "-c", _PEAK_REPORTER, *arguments],
        capture_output=True,
        check=True,
        text=True,
    )

    return int(completed.stdout.strip())


@pytest.mark.skipif(
    sys.platform != "linux",
    reason="`/proc/self/status` is where a process reports its own peak, and it is Linux only",
)
def test_fingerprinting_a_long_recording_holds_only_its_result(tmp_path: Path) -> None:
    recording: Path = tmp_path / "long.wav"
    _write_recording(recording)

    baseline_peak = _peak_kibibytes()
    fingerprint_peak = _peak_kibibytes(recording)

    assert fingerprint_peak - baseline_peak < _PEAK_ALLOWANCE_KIB
