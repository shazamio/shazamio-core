# The one home of every command this project runs. A CI job and a local check are
#  the same string here rather than two copies that drift apart.

# `bash` on all three CI runners and on a developer machine, so a recipe behaves
#  the same everywhere. The default is `sh`, which on Windows is whatever Git
#  happens to have put on `PATH`.
set shell := ["bash", "-uc"]

[doc("Show the recipes")]
default:
    @just --list

[doc("Build the extension into the environment and install the test dependencies")]
sync:
    uv sync

[doc("Run the Python test suite")]
test:
    uv run pytest

# Nothing else compares the hand-written `.pyi` with the extension, and it had
#  drifted: `Recognizer.__init__` declared a parameter the runtime carries on
#  `__new__`, and three classes claimed to be `@dataclass`.
[doc("Check the type stub against the built extension")]
stubtest:
    uv run python -m mypy.stubtest shazamio_core.shazamio_core

[doc("Run the Rust unit tests")]
rust-test:
    cargo test

[doc("Reformat the crate")]
fmt:
    cargo fmt --all

# `--all-targets` is what reaches the `#[cfg(test)]` modules. The default target
#  set stops at the library, so every unit test would go unlinted.
#  https://doc.rust-lang.org/cargo/commands/cargo-clippy.html#target-selection
[doc("Check the formatting and run `clippy`")]
lint:
    cargo fmt --all --check
    cargo clippy --all-targets -- -D warnings

# Everything else builds with current stable, so a `cargo update` can raise the
#  real floor and stay green. Whoever builds the sdist on a distro toolchain is
#  the one who finds out.
[doc("Check the crate against the Rust version it declares")]
msrv:
    #!/usr/bin/env bash
    set -euo pipefail

    # Read from `Cargo.toml` rather than restated here: a second copy would drift
    #  and leave the check running against a floor the crate no longer declares.
    version="$(sed -n 's/^rust-version = "\(.*\)"/\1/p' Cargo.toml)"

    rustup toolchain install "$version" --profile minimal
    cargo "+$version" check --locked --all-targets

[doc("Everything CI gates on; the first run downloads the MSRV toolchain")]
all: lint rust-test test stubtest msrv
