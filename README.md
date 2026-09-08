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

Everything else builds from the source distribution, which needs a Rust toolchain 1.87 or newer, CMake and a C compiler. `libopus` is built from vendored sources rather than linked against a system copy, so without CMake the build stops on `is 'cmake' not installed?`.

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

`recognize_path` accepts a `str` or an `os.PathLike[str]`. A `__fspath__` returning `bytes` is rejected.

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
import asyncio

from shazamio_core import Recognizer, SearchParams


async def main() -> None:
    recognizer = Recognizer(segment_duration_seconds=5)

    signature = await recognizer.recognize_path(
        "track.mp3",
        SearchParams(segment_duration_seconds=15),
    )

    print(signature.signature.samples)


asyncio.run(main())
```

`SearchParams` wins where both are given. The duration must be at least 1; zero
raises `ValueError`. A value at or above the length of the file analyses it whole,
whatever the value.

### Errors

Audio that cannot be decoded, and a file that is not there, raise `SignatureError`:

```python
import asyncio

from shazamio_core import Recognizer, SignatureError


async def main() -> None:
    try:
        await Recognizer().recognize_path("not-audio.txt")
    except SignatureError as error:
        print(error)


asyncio.run(main())
```

## Formats

Decoding goes through [`symphonia`](https://github.com/pdeljanov/Symphonia) with every codec and container it ships enabled, and resampling to mono 16 kHz through [`rubato`](https://github.com/HEnquist/rubato). Opus is the one codec `symphonia` has no decoder for, so it goes through [`libopus`](https://github.com/SpaceManiac/opus-rs), which is compiled into the wheel rather than loaded from the system. Nothing is shelled out to, so no external binary has to be installed.

That leaves these codecs, each one probed through the public API in the container named beside it:

| Codec  | Probed in      | In the test suite |
|--------|----------------|-------------------|
| AAC    | ADTS           |                   |
| ADPCM  | WAV            |                   |
| ALAC   | MP4            |                   |
| FLAC   | FLAC           | yes               |
| MP1    | not probed     |                   |
| MP2    | MPEG           |                   |
| MP3    | MPEG           | yes               |
| Opus   | Ogg            | yes               |
| PCM    | WAV, AIFF, CAF |                   |
| Vorbis | Ogg            | yes               |

MP1 is the one row taken from what `symphonia` registers rather than from a run: nothing here encodes it. The containers recognised are ADTS, AIFF, CAF, FLAC, Matroska and WebM, MP4, MPEG, Ogg and WAV, and the test suite runs on Linux, macOS and Windows.

Anything else raises `SignatureError`, and a container from that list is no guarantee: what has to be decodable is the codec inside it. The two refusals differ in how far the file gets:

| Refused          | Error                        | Why                                     |
|------------------|------------------------------|-----------------------------------------|
| AC-3 in Matroska | `unsupported feature: codec` | the container is read, the codec is not |
| WMA in ASF       | `end of stream`              | there is no ASF demuxer at all          |

Windows Media Audio decoded in earlier releases through an `ffmpeg` fallback that has since been removed.

## Development

Every check CI runs is a [`just`](https://github.com/casey/just) recipe, so the two cannot drift apart:

```sh
just --list      # what there is
just install     # builds the extension, installs the test dependencies, `cargo-about` and the commit hooks
just all         # everything CI gates on
```

`just install` also wires the same recipes into `git commit` through [`pre-commit`](https://pre-commit.com), each one scoped to the files it gates, so a change to the `README` runs none of them and a change to the crate runs all of them. CI scopes its jobs the same way, from the same sets: `.github/path-filters.yaml`.

`just install` needs the toolchain the Install section lists; `maturin` comes from `pyproject.toml` and is fetched automatically. `just` itself is packaged for most systems, listed under [Packages](https://github.com/casey/just#packages).

`just rust-test` links `libpython`, so on Debian and Ubuntu the development package of the interpreter `cargo` picks up has to be present, or the build stops at `rust-lld: error: unable to find library -lpython3.14`:

```sh
sudo apt install libpython3.14-dev
```

## License

MIT. See [LICENSE](LICENSE).

The wheel statically links its Rust dependencies, so the terms in
[THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md) apply to it as well. That file
says how it is generated; `just licenses-check` fails once a dependency change
has left it behind.
