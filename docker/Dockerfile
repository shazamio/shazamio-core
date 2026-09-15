# check=skip=InvalidDefaultArgInFrom

# The base is an argument with no default: both tags live in the `linux` matrix of
#  `.github/workflows/ci.yaml`, so a bare `docker build` fails rather than drifting
#  from what CI publishes. Reporting that is what the check above does.
#  https://docs.docker.com/reference/build-checks/invalid-default-arg-in-from/
#  The two images lay their interpreters out alike and differ only in their package
#  manager, which `install-build-deps.sh` handles. Neither tag names an architecture:
#  each is a manifest list, so buildx pulls the entry matching `platforms`.
#  https://docs.docker.com/build/building/multi-platform/
ARG BASE_IMAGE
FROM ${BASE_IMAGE} AS base

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
 && pip3 install "maturin==${MATURIN_VERSION#v}" patchelf

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
