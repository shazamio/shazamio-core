# The one home of every command this project runs. A CI job and a local check are
#  the same string here rather than two copies that drift apart.

# `bash` on all three CI runners and on a developer machine, so a recipe behaves
#  the same everywhere. The default is `sh`, which on Windows is whatever Git
#  happens to have put on `PATH`.
set shell := ["bash", "-uc"]

# An unpinned generator rewrites the notices and turns a green branch red with
#  nobody having touched the tree. The `Licence notices` job reads this value with
#  `just --evaluate` rather than restating it, so the version has one home.
cargo_about_version := "0.9.2"

# Named once so the recipe that writes the notices and the one that diffs them
#  cannot disagree about which file that is. `pyproject.toml` names it too, in
#  `license-files`, because a manifest cannot read a recipe.
notices := "THIRD-PARTY-NOTICES.md"

[doc("Show the recipes")]
default:
    @just --list

[doc("Build the extension into the environment and install the test dependencies")]
sync:
    uv sync

# `--install-hooks` builds the hook environments now instead of during whichever
#  commit happens to be the first, which otherwise stalls for a minute with no
#  indication that it is downloading rather than checking.

# Without `--features cli` the install builds no executable at all and says so
#  only in a warning, because the `cargo-about` binary sits behind that feature.
#  https://github.com/EmbarkStudios/cargo-about/blob/f7394d5c8f618623573072caadf6594821c789b6/Cargo.toml#L23-L26
[doc("Everything a checkout needs: the dependencies, `cargo-about` and the `pre-commit` hooks")]
install: sync
    cargo install cargo-about --locked --features cli --version '={{ cargo_about_version }}'
    uv run pre-commit install --install-hooks

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

# What the wheel ships beside `LICENSE`. Some crates carry their licence with
#  CRLF or a trailing blank line, which the whitespace hooks rewrite: the diff
#  below then fails on a later commit with no dependency having moved.
[doc("Regenerate the third-party licence notices from `Cargo.lock`")]
licenses output=notices:
    #!/usr/bin/env bash
    set -euo pipefail

    cargo about generate --config licenses/about.toml licenses/notices.hbs --output-file {{ output }}

    normalized="$(mktemp)"
    trap 'rm -f "$normalized"' EXIT

    # `$(...)` drops every trailing newline, so the `printf` leaves exactly one.
    printf '%s\n' "$(sed 's/[[:space:]]*$//' {{ output }})" > "$normalized"
    cp "$normalized" {{ output }}

[doc("Check the committed notices still match `Cargo.lock`")]
licenses-check:
    #!/usr/bin/env bash
    set -euo pipefail

    generated="$(mktemp)"
    trap 'rm -f "$generated"' EXIT

    just licenses "$generated"
    diff -u {{ notices }} "$generated"

[doc("Everything CI gates on; the first run downloads the MSRV toolchain")]
all: lint rust-test test stubtest msrv licenses-check
