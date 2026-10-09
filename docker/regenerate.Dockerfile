# The one environment `tests/data` is generated in, so a rerun anywhere writes the
#  committed bytes. Both images are pinned by digest: the lossy fixtures depend on
#  the encoder libraries inside the `ffmpeg` build, and the golden URIs on the `libm`
#  of the Rust image's glibc. Moving either digest means `just regenerate`, then
#  committing whatever it rewrites.

# A static build, so the encoder libraries travel inside the binary and not with
#  whichever distro it lands on. Always the x86-64 one, which `qemu-x86_64` below
#  runs on an arm64 host too, so that host writes the same bytes.
FROM --platform=linux/amd64 mwader/static-ffmpeg:8.0.1@sha256:252705ff88532fa338e7065c21792756552f8fe7c212f84bc503d3c340689594 AS ffmpeg

FROM rust:1.99.0-slim-trixie@sha256:24e632c09342c20abf8312cf4f61430a911c01ed3a5e4c02b87292b1c39c5273

# The test binary is an executable rather than an extension module, so it links
#  `libpython`, which `python3-dev` carries. `opusic-sys` builds the bundled
#  `libopus` through `cmake`, which needs `make` and a C++ compiler on top.
RUN apt-get update \
 && apt-get install --yes --no-install-recommends cmake g++ make python3-dev qemu-user \
 && rm -rf /var/lib/apt/lists/*

# `libopus` picks its SIMD code from CPUID at runtime, which `-cpuflags` does not
#  reach, so the Opus fixtures came out differently on different CI runner CPUs.
#  Under `qemu` the CPU is a fixed model and its float ops are computed in
#  software, so every host writes the same bytes.
#  https://gitlab.com/qemu-project/qemu/-/blob/7c949c53e936aa3a658d84ab53bae5cadaa5d59c/target/i386/ops_sse.h#L514-L517
COPY --from=ffmpeg /ffmpeg /usr/local/libexec/ffmpeg
RUN printf '#!/bin/sh\nexec qemu-x86_64 -cpu qemu64 /usr/local/libexec/ffmpeg "$@"\n' > /usr/local/bin/ffmpeg \
 && chmod +x /usr/local/bin/ffmpeg
