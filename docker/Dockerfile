# check=skip=InvalidDefaultArgInFrom

# `BASE_IMAGE` names one of the stages below and has no default, so a bare
#  `docker build` fails rather than guessing which wheel to build; the `linux`
#  matrix of `.github/workflows/ci.yaml` passes it. Reporting that is what the check
#  above does. Only the named stage is built, so the other images are never pulled.
#  https://docs.docker.com/reference/build-checks/invalid-default-arg-in-from/
#  The images lay their interpreters out alike, and all carry the `cmake` and
#  C compiler that the bundled `libopus` build needs. No tag names an architecture:
#  each is a manifest list, so buildx pulls the entry matching `platforms`.
#  https://docs.docker.com/build/building/multi-platform/
#  Tagged here rather than in the workflow, where Dependabot reads no image.
#
# `manylinux_2_28` has no riscv64 image, so the glibc riscv64 wheel comes from
#  `manylinux_2_39`, the only one that has. The wheel still reads `manylinux_2_34`:
#  `maturin` tags it with the oldest policy whose symbol versions the binary meets.
#  https://github.com/PyO3/maturin/blob/3bec0a32ae2b67ad4fac0194f762f44141e51473/src/auditwheel/linux.rs#L327-L365
#  https://github.com/pypa/manylinux/blob/89b4e2b712e83be32ba37c20c37069702620c877/README.rst?plain=1#L37-L47
ARG BASE_IMAGE
FROM quay.io/pypa/manylinux_2_28:2026.10.31-1 AS manylinux
FROM quay.io/pypa/manylinux_2_39:2026.10.31-1 AS manylinux_2_39
FROM quay.io/pypa/musllinux_1_2:2026.10.31-1 AS musllinux
FROM ${BASE_IMAGE} AS base

# Only the minimum supported interpreter belongs here: the bare build in
#  `docker/build-wheels.sh` takes it for the `abi3` wheel, which a newer one may not
#  build (argued above `pyo3` in `Cargo.toml`), and `pip3` below needs it. Must match
#  the `abi3-py*` feature in `Cargo.toml`.
ENV PATH="/root/.cargo/bin:/opt/python/cp311-cp311/bin:$PATH"
# The one pin lives in `tools/Cargo.toml` and arrives here as a build argument, so
#  the Linux wheels and the macOS/Windows wheels of a release cannot come out of
#  different `maturin` versions. Unset is a hard error rather than a default: a
#  default is the second copy this removes.
ARG MATURIN_VERSION
RUN : "${MATURIN_VERSION:?the MATURIN_VERSION build argument is required}" \
 && pip3 install "maturin==${MATURIN_VERSION}" patchelf

WORKDIR /opt
COPY docker/install-build-deps.sh .
RUN bash /opt/install-build-deps.sh

FROM base AS chef
ENV PATH="/root/.cargo/bin:$PATH"
RUN rustc -V

FROM chef AS planner
WORKDIR /opt
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
WORKDIR /opt
COPY --from=planner /opt/recipe.json recipe.json
# `cargo chef` runs plain `cargo` before `maturin` is ever invoked, so nothing turns
#  extension-module linking on for it, and no interpreter in this image ships a
#  `libpython` -- the step died on `could not find native static library python3.9`,
#  naming whichever `cpXY` led `PATH` when that was written.
#  Why the feature went away: the `[build-system]` comment in `pyproject.toml`.
RUN PYO3_BUILD_EXTENSION_MODULE=1 cargo chef cook --release --recipe-path recipe.json
COPY . .
RUN bash /opt/docker/build-wheels.sh
RUN rm -rf target

# Pinned to a stable line, not `latest`: this stage only carries the wheels out,
#  and a silent Alpine major bump underneath them buys nothing.
FROM alpine:3.24 AS dist
COPY --from=builder /opt/dist /opt/dist
