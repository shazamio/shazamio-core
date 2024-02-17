from dataclasses import dataclass


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


class SignatureError(Exception):
    def __init__(self, message: str):
        self.message = message

    def __str__(self) -> str:
        return self.message

    def __repr__(self) -> str:
        return f"SignatureError({self.message})"


class Recognizer:
    async def recognize_path(self, value: str) -> Signature:
        """
        :param value: path file
        :return: Signature object
        :raises SignatureError: if there is any error
        """
        raise NotImplemented

    async def recognize_bytes(self, value: bytes) -> Signature:
        """
        :param value: bytes file
        :return: Signature object
        :raises SignatureError: if there is any error
        """
        raise NotImplemented
