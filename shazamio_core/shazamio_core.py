from dataclasses import dataclass
from typing import Union, Optional
from os import PathLike


@dataclass
class Geolocation:
    altitude: int
    latitude: int
    longitude: int


@dataclass
class SignatureSong:
    samples: int
    timestamp: int
    uri: str


@dataclass
class Signature:
    geolocation: Geolocation
    signature: SignatureSong
    timestamp: int
    timezone: str


@dataclass(frozen=True)
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
    segment_duration_seconds: int = 10


class SignatureError(Exception):
    def __init__(self, message: str):
        self.message = message

    def __str__(self) -> str:
        return self.message

    def __repr__(self) -> str:
        return f"SignatureError({self.message})"


class Recognizer:
    """
    Recognizer uses a Rust implementation under the hood.

    This class provides an interface for recognizing audio files, but the actual
    processing logic is implemented in Rust and accessed via FFI.
    """

    def __init__(self, segment_duration_seconds: int = 10) -> None:
        """
        :param segment_duration_seconds: The duration (in seconds) of the audio segment to analyze.
            - **Default:** 12 seconds.
            - **If the audio file is longer than this duration**, a centered segment of the specified duration is selected.
              - Example: If the audio is **60 seconds** and `segment_duration_seconds = 10`, the extracted segment will be **from 25s to 35s**.
            - **If the audio file is shorter than this duration**, the entire file is used.
              - Example: If the audio is **8 seconds** and `segment_duration_seconds = 10`, the entire **8-second file** will be processed.
            - **Audio is always converted to mono and down sampled to 16 kHz** before analysis.
            - This parameter determines the number of samples used for frequency analysis and fingerprint generation.
        """
        self.segment_duration_seconds = segment_duration_seconds
        raise NotImplemented

    async def recognize_path(
            self,
            value: Union[str, PathLike],
            options: Optional[SearchParams] = None,
    ) -> Signature:
        """
        Recognize audio from a file path.

        This method is a Python wrapper around a Rust implementation.

        :param value: Path to an audio file.
        :param options: Search parameters.
        :return: Signature object.
        :raises SignatureError: if an error occurs.
        """
        raise NotImplemented

    async def recognize_bytes(
            self,
            value: bytes,
            options: Optional[SearchParams] = None,
    ) -> Signature:
        """
        Recognize audio from raw bytes.

        This method is a Python wrapper around a Rust implementation.

        :param value: Raw audio file as bytes.
        :param options: Search parameters.
        :return: Signature object.
        :raises SignatureError: if an error occurs.
        """
        raise NotImplemented
