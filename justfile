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

# Pinned for the same reason, and from PyPI so a checkout needs no second package
#  manager to run them.
shellcheck_version := "0.11.0.1"
actionlint_version := "1.7.12.24"

# Named once so the recipe that writes the notices and the one that diffs them
#  cannot disagree about which file that is. `pyproject.toml` names it too, in
#  `license-files`, because a manifest cannot read a recipe.
notices := "THIRD-PARTY-NOTICES.md"

[doc("Show the recipes")]
default:
    @just --list

# --- Setup ---

# Separate from `install` because the CI test matrix wants the environment and
#  nothing else: `cargo-about` and the hook environments cost minutes on each of
#  the three runners, and neither is used to run a test.
[doc("The environment the tests need: the dependencies and the built extension")]
install-test:
    uv sync

# `--install-hooks` builds the hook environments now instead of during whichever
#  commit happens to be the first, which otherwise stalls for a minute with no
#  indication that it is downloading rather than checking.

# Without `--features cli` the install builds no executable at all and says so
#  only in a warning, because the `cargo-about` binary sits behind that feature.
#  https://github.com/EmbarkStudios/cargo-about/blob/f7394d5c8f618623573072caadf6594821c789b6/Cargo.toml#L23-L26
[doc("Everything a checkout needs: the dependencies, `cargo-about` and the `pre-commit` hooks")]
install: install-test
    cargo install cargo-about --locked --features cli --version '={{ cargo_about_version }}'
    uv run pre-commit install --install-hooks

# --- Code quality ---

[doc("Every check that reads and never writes")]
lint: lint-rust lint-harness

# `--all-targets` is what reaches the `#[cfg(test)]` modules. The default target
#  set stops at the library, so every unit test would go unlinted.
#  https://doc.rust-lang.org/cargo/commands/cargo-clippy.html#target-selection
[doc("Check the formatting and run `clippy`")]
lint-rust:
    cargo fmt --all --check
    cargo clippy --all-targets -- -D warnings

# Both tools come from one environment because `actionlint` shells out to
#  `shellcheck` for every `run:` block and finds it on `PATH`.
#  https://github.com/rhysd/actionlint/blob/914e7df21a07ef503a81201c76d2b11c789d3fca/docs/checks.md#shellcheck-integration-for-run
#  Given no path it globs the workflows itself. Given one it reads that file as a
#  workflow, which `.github/actions/*/action.yml` is not: every key of a composite
#  action is then reported as unexpected.
[doc("Lint the scripts and the workflows, and check what the gate and the filters cover")]
lint-harness:
    uv run --no-project --with 'shellcheck-py=={{ shellcheck_version }}' shellcheck $(git ls-files '*.sh')
    uv run --no-project --with 'shellcheck-py=={{ shellcheck_version }}' --with 'actionlint-py=={{ actionlint_version }}' actionlint
    uv run --no-project --with pyyaml python scripts/check_ci_gate.py
    uv run --no-project --with pyyaml python scripts/check_ci_filters.py

[doc("Reformat the crate")]
format:
    cargo fmt --all

# Nothing else compares the hand-written `.pyi` with the extension, and it had
#  drifted: `Recognizer.__init__` declared a parameter the runtime carries on
#  `__new__`, and three classes claimed to be `@dataclass`.

# `stubtest` compares declarations and never reads a call site, so what a caller
#  may do with what the entry points return is checked by type-checking one.
[doc("Check the type stub against the built extension, and against a caller")]
typecheck:
    uv run python -m mypy.stubtest shazamio_core.shazamio_core
    uv run mypy --strict tests/test_typing.py

# --- Tests ---

[doc("Run both suites")]
test: test-rust test-python

[doc("Run the Python suite; extra arguments reach `pytest`")]
test-python *args:
    uv run pytest {{ args }}

[doc("Run the Rust suite; extra arguments reach `cargo test`")]
test-rust *args:
    cargo test {{ args }}

# --- Release ---

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

# Nothing else opens a release archive, and every way of getting one wrong is
#  silent: a wheel without the stub type-checks as `Any`, a source archive
#  without `licenses/` fails nothing until `licenses-check` runs inside it, and
#  a long description PyPI cannot render is reported at upload or not at all.
#  What ships is decided by `include` in `Cargo.toml`, `license-files` in
#  `pyproject.toml` and `maturin` itself, none of which can see the others.
[doc("Check the built artifacts in a directory carry and declare what they should")]
dist-check directory="dist":
    uv run --no-project python scripts/check_dist.py {{ quote(directory) }}
    uv run --no-project --with twine twine check {{ quote(directory) }}/*

# What a user receives, as opposed to what a wheel contains: nothing else here
#  installs one, and nothing else loads the extension it carries.
[doc("Install a built wheel outside the checkout and import what it carries")]
dist-smoketest directory="dist":
    uv run --no-project python scripts/smoketest_dist.py {{ quote(directory) }}

# The recipe above cannot answer for a musl wheel: `uv` resolves by platform tag, so
#  installing one on the glibc machine that built it fails outright, naming
#  `musllinux_1_2_x86_64` as the platform it is for. The same script runs inside
#  Alpine instead, in an image carrying `uv`, a `python` and no compiler, pinned by
#  digest for the reason every `uses:` in the workflow is. The wheels mount apart
#  from the checkout, which the script needs as a directory to compare against.
[doc("Install a built musl wheel inside Alpine and import what it carries")]
dist-smoketest-musl directory="dist":
    docker run --rm \
        --volume {{ quote(justfile_directory()) }}:/src:ro \
        --volume "$(cd {{ quote(directory) }} && pwd)":/wheels:ro \
        --workdir /src \
        ghcr.io/astral-sh/uv:python3.12-alpine@sha256:bba3bd4965901846f025ae7375402ba859862bb83181f19d7e4a82f0f1f8388e \
        python scripts/smoketest_dist.py /wheels

# --- CI ---

[doc("Everything CI gates on; the first run downloads the MSRV toolchain")]
ci: lint typecheck test msrv licenses-check
