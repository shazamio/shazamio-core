# shazamio-core

Audio fingerprinting for Shazam, written in Rust and exposed to Python.

It turns an audio file, or its bytes, into the signature Shazam's endpoint accepts. It does not talk to Shazam itself: [ShazamIO](https://github.com/shazamio/ShazamIO) is the client that sends the signature and reads the answer back.

## Install

```sh
pip install shazamio-core
```

Or with [uv](https://docs.astral.sh/uv/):

```sh
uv add shazamio-core
```

Python 3.10 and newer. Prebuilt wheels:

| Platform                                     | CPython 3.10+ (`abi3`) | PyPy 3.11 |
|----------------------------------------------|------------------------|-----------|
| Linux `x86_64`, `aarch64` (`manylinux_2_28`) | yes                    | yes       |
| macOS `x86_64` (10.12+), `arm64` (11.0+)     | yes                    | no        |
| Windows `win_amd64`, `win32`                 | yes                    | no        |

Everything else builds from the source distribution and needs a Rust toolchain, 1.87 or newer.

## Usage

Both entry points are coroutines.

```python
import asyncio
from pathlib import Path

from shazamio_core import Recognizer


async def main() -> None:
    recognizer = Recognizer()

    from_path = await recognizer.recognize_path("track.mp3")
    from_bytes = await recognizer.recognize_bytes(Path("track.mp3").read_bytes())

    print(from_path.signature.uri == from_bytes.signature.uri)


asyncio.run(main())
```

`recognize_path` accepts a `str` or any `os.PathLike`.

### What comes back

Both return a `Signature`:

| Field                                  | Meaning                                                                                                     |
|----------------------------------------|-------------------------------------------------------------------------------------------------------------|
| `signature.uri`                        | the fingerprint itself, base64 inside a `data:audio/vnd.shazam.sig` URI                                     |
| `signature.samples`                    | duration of the analysed segment in milliseconds                                                            |
| `signature.timestamp`                  | when the signature was produced                                                                             |
| `timestamp`, `timezone`, `geolocation` | fixed values the request envelope carries. They are not read from the machine and mean nothing on their own |

The URI is the part a client sends on:

```
data:audio/vnd.shazam.sig;base64,gCX+ypQoAnWcBQAAAJwRlAAAAAA...
```

### How much audio is analysed

Ten seconds by default, taken from the middle of the file. A file shorter than the segment is used whole. Audio is converted to mono and downsampled to 16 kHz before analysis, whatever it started as.

Set it per recognizer, or per call:

```python
recognizer = Recognizer(segment_duration_seconds=5)

signature = await recognizer.recognize_path(
    "track.mp3",
    SearchParams(segment_duration_seconds=15),
)
```

`SearchParams` wins where both are given.

### Errors

Audio that cannot be decoded, and a file that is not there, raise `SignatureError`:

```python
from shazamio_core import Recognizer, SignatureError

try:
    await Recognizer().recognize_path("not-audio.txt")
except SignatureError as error:
    print(error)
```

## Formats

Decoding goes through [`rodio`](https://github.com/RustAudio/rodio) and `symphonia`, with the `flac`, `mp3`, `mp4`, `vorbis` and `wav` features enabled. The test suite covers MP3, Ogg Vorbis and FLAC on Linux, macOS and Windows.

## Development

Every check CI runs is a [`just`](https://github.com/casey/just) recipe, so the two cannot drift apart:

```sh
just --list      # what there is
just sync        # builds the extension and installs the test dependencies
just all         # everything CI gates on
```

`just sync` needs a Rust toolchain; `maturin` comes from `pyproject.toml` and is fetched automatically. `just` itself is packaged for most systems, listed under [Packages](https://github.com/casey/just#packages).

`just rust-test` links `libpython`, so on Debian and Ubuntu the development package of the interpreter `cargo` picks up has to be present, or the build stops at `rust-lld: error: unable to find library -lpython3.14`:

```sh
sudo apt install libpython3.14-dev
```

## License

MIT. See [LICENSE](LICENSE).
