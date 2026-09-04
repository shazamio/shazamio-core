# The tag names no architecture on purpose: it is a manifest list, so buildx pulls the
#  entry matching `platforms` in the workflow and one file serves both runners. The date
#  is pinned because a silent rebuild of the toolchain that links the wheel would ship
#  unnoticed.
#  https://docs.docker.com/build/building/multi-platform/
FROM quay.io/pypa/manylinux_2_28:2026.08.24-1 AS base

RUN dnf install -y epel-release
# RUN dnf config-manager --set-enabled crb
RUN dnf install -y --nogpgcheck https://download1.rpmfusion.org/free/el/rpmfusion-free-release-8.noarch.rpm
RUN dnf install -y --nogpgcheck https://download1.rpmfusion.org/nonfree/el/rpmfusion-nonfree-release-8.noarch.rpm
RUN dnf install -y clang clang-devel
# Only the minimum supported interpreter belongs here: the wheel is `abi3`, so one
#  is emitted whatever builds it, and this entry stays only so `pip3` below exists
#  at all. Must match the `abi3-py*` feature in `Cargo.toml`.
ENV PATH="/root/.cargo/bin:/opt/python/cp310-cp310/bin:$PATH"
# The one pin lives in `MATURIN_VERSION` in `.github/workflows/ci.yaml` and arrives
#  here as a build argument, so the Linux wheels and the macOS/Windows wheels of a
#  release cannot come out of different `maturin` versions. Unset is a hard error
#  rather than a default: a default is the second copy this removes. The tag carries
#  a `v` that no other site uses, so it is stripped.
ARG MATURIN_VERSION
RUN : "${MATURIN_VERSION:?the MATURIN_VERSION build argument is required}" \
 && pip3 install "maturin==${MATURIN_VERSION#v}" patchelf cffi ziglang sccache>=0.4.0

WORKDIR /opt
COPY docker/install-basic-deps-manylinux.sh .
RUN bash /opt/install-basic-deps-manylinux.sh

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
ARG PYTHON_INTERPRETER
RUN bash /opt/docker/build-manylinux.sh
RUN rm -rf target

# Pinned to a stable line, not `latest`: this stage only carries the wheels out,
# and a silent Alpine major bump underneath them buys nothing.
FROM alpine:3.24 AS dist
COPY --from=builder /opt/dist /opt/dist
