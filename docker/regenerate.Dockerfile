# The one environment `tests/data` is generated in, so a rerun anywhere writes the
#  committed bytes. Both images are pinned by digest: the lossy fixtures depend on
#  the encoder libraries inside the `ffmpeg` build, and the golden URIs on the `libm`
#  of the Rust image's glibc. Moving either digest means `just regenerate`, then
#  committing whatever it rewrites.

# A static build, so the encoder libraries travel inside the binary and not with
#  whichever distro it lands on.
FROM mwader/static-ffmpeg:8.0.1@sha256:252705ff88532fa338e7065c21792756552f8fe7c212f84bc503d3c340689594 AS ffmpeg

FROM rust:1.99.0-slim-trixie@sha256:24e632c09342c20abf8312cf4f61430a911c01ed3a5e4c02b87292b1c39c5273

# The test binary is an executable rather than an extension module, so it links
#  `libpython`, which `python3-dev` carries. `opusic-sys` builds the bundled
#  `libopus` through `cmake`, which needs `make` and a C++ compiler on top.
RUN apt-get update \
 && apt-get install --yes --no-install-recommends cmake g++ make python3-dev \
 && rm -rf /var/lib/apt/lists/*

COPY --from=ffmpeg /ffmpeg /usr/local/bin/ffmpeg
