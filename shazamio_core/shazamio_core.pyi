"""Type stub for the `shazamio_core` extension, written by hand.

`maturin --generate-stubs` cannot produce it: the introspection behind it only
covers a declarative `#[pymodule] mod`, and on a function-form module like ours it
emits a `__getattr__` returning `Incomplete`, which types every name as `Any`.
https://github.com/PyO3/pyo3/blob/v0.29.2/guide/src/type-stub.md

`mypy.stubtest` checks this file against the compiled module in CI.
"""

from os import PathLike
from typing import final

__all__ = [
    "Geolocation",
    "Recognizer",
    "SearchParams",
    "Signature",
    "SignatureError",
    "SignatureSong",
]


@final
class Geolocation:
    altitude: int
    latitude: int
    longitude: int

    def __new__(cls, altitude: int, latitude: int, longitude: int) -> Geolocation: ...


@final
class SignatureSong:
    samples: int
    timestamp: int
    uri: str

    def __new__(cls, samples: int, timestamp: int, uri: str) -> SignatureSong: ...


@final
class Signature:
    geolocation: Geolocation
    signature: SignatureSong
    timestamp: int
    timezone: str

    def __new__(
        cls,
        geolocation: Geolocation,
        signature: SignatureSong,
        timestamp: int,
        timezone: str,
    ) -> Signature: ...


@final
class SearchParams:
    """
    Search parameters for the recognize method.

    **segment_duration_seconds**: The duration (in seconds) of the audio segment to analyze.
        - **Default:** 10 seconds.
        - **If the audio file is longer than this duration**, a centered segment of the specified duration is selected.
          - Example: If the audio is **60 seconds** and `segment_duration_seconds = 10`, the extracted segment will be **from 25s to 35s**.
        - **If the audio file is shorter than this duration**, the entire file is used.
          - Example: If the audio is **8 seconds** and `segment_duration_seconds = 10`, the entire **8-second file** will be processed.
        - **Audio is always converted to mono and down sampled to 16 kHz** before analysis.
        - This parameter determines the number of samples used for frequency analysis and fingerprint generation.
    """

    segment_duration_seconds: int

    def __new__(cls, segment_duration_seconds: int | None = None) -> SearchParams: ...


class SignatureError(Exception): ...


@final
class Recognizer:
    """
    Recognizer uses a Rust implementation under the hood.

    This class provides an interface for recognizing audio files, but the actual
    processing logic is implemented in Rust and accessed via FFI.
    """

    segment_duration_seconds: int

    def __new__(cls, segment_duration_seconds: int | None = None) -> Recognizer:
        """
        :param segment_duration_seconds: The duration (in seconds) of the audio segment to analyze.
            - **Default:** 10 seconds.
            - **If the audio file is longer than this duration**, a centered segment of the specified duration is selected.
              - Example: If the audio is **60 seconds** and `segment_duration_seconds = 10`, the extracted segment will be **from 25s to 35s**.
            - **If the audio file is shorter than this duration**, the entire file is used.
              - Example: If the audio is **8 seconds** and `segment_duration_seconds = 10`, the entire **8-second file** will be processed.
            - **Audio is always converted to mono and down sampled to 16 kHz** before analysis.
            - This parameter determines the number of samples used for frequency analysis and fingerprint generation.
        """

    async def recognize_path(
        self,
        value: str | PathLike[str],
        options: SearchParams | None = None,
    ) -> Signature:
        """
        Recognize audio from a file path.

        This method is a Python wrapper around a Rust implementation.

        :param value: Path to an audio file.
        :param options: Search parameters.
        :return: Signature object.
        :raises SignatureError: if an error occurs.
        """

    async def recognize_bytes(
        self,
        value: bytes,
        options: SearchParams | None = None,
    ) -> Signature:
        """
        Recognize audio from raw bytes.

        This method is a Python wrapper around a Rust implementation.

        :param value: Raw audio file as bytes.
        :param options: Search parameters.
        :return: Signature object.
        :raises SignatureError: if an error occurs.
        """
