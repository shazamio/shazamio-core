"""Type stub for the `shazamio_core` extension, written by hand.

`maturin --generate-stubs` cannot produce it: the introspection behind it only
covers a declarative `#[pymodule] mod`, and on a function-form module like ours it
emits a `__getattr__` returning `Incomplete`, which types every name as `Any`.
https://github.com/PyO3/pyo3/blob/v0.29.2/guide/src/type-stub.md

`mypy.stubtest` checks this file against the compiled module in CI.
"""

from collections.abc import Awaitable
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


# Every field of the three classes below is a getter and nothing more:
#  `src/response.rs` declares them `#[pyo3(get)]`, so the runtime refuses an
#  assignment that a plain attribute here would let a caller write.
#  `tests/test_typing.py` holds the declarations to that.
@final
class Geolocation:
    @property
    def altitude(self) -> int: ...
    @property
    def latitude(self) -> int: ...
    @property
    def longitude(self) -> int: ...

    def __new__(cls, altitude: int, latitude: int, longitude: int) -> Geolocation: ...


@final
class SignatureSong:
    @property
    def samples(self) -> int: ...
    @property
    def timestamp(self) -> int: ...
    @property
    def uri(self) -> str: ...

    def __new__(cls, samples: int, timestamp: int, uri: str) -> SignatureSong: ...


@final
class Signature:
    @property
    def geolocation(self) -> Geolocation: ...
    @property
    def signature(self) -> SignatureSong: ...
    @property
    def timestamp(self) -> int: ...
    @property
    def timezone(self) -> str: ...

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
        - **Must be at least 1.** Zero raises `ValueError`, at the constructor and on assignment.
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

    Both recognize methods return an `asyncio.Future`, not a coroutine: they need a
    running event loop at the call, and the work starts there rather than at the
    `await`. The `README` says what that allows and what it rules out.
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
            - **Must be at least 1.** Zero raises `ValueError`, at the constructor and on assignment.
        """

    # The runtime hands back `loop.create_future()` and spawns the work straight
    #  after, so what these return is an awaitable and never a coroutine.
    #  `tests/test_typing.py` holds the declaration below to what a caller may do.
    #  https://github.com/PyO3/pyo3-async-runtimes/blob/58d42b7a3eb239719175c5587b2b7debd9ee134b/src/generic.rs#L620-L631
    def recognize_path(
        self,
        value: str | PathLike[str],
        options: SearchParams | None = None,
    ) -> Awaitable[Signature]:
        """
        Recognize audio from a file path.

        This method is a Python wrapper around a Rust implementation.

        :param value: Path to an audio file.
        :param options: Search parameters.
        :return: `Awaitable` resolving to a `Signature`.
        :raises SignatureError: if an error occurs.
        """

    def recognize_bytes(
        self,
        value: bytes,
        options: SearchParams | None = None,
    ) -> Awaitable[Signature]:
        """
        Recognize audio from raw bytes.

        This method is a Python wrapper around a Rust implementation.

        :param value: Raw audio file as bytes.
        :param options: Search parameters.
        :return: `Awaitable` resolving to a `Signature`.
        :raises SignatureError: if an error occurs.
        """
