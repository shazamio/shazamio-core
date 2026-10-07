# Contributing

## Setting up a checkout

Every check CI runs is a [`just`](https://github.com/casey/just) recipe, so the two cannot drift apart:

```sh
just --list      # what there is
just install     # builds the extension, installs the test dependencies, `cargo-about` and the commit hooks
just ci          # everything CI gates on
```

`just install` needs the toolchain the [Install](README.md#install) section lists; `maturin` comes from `pyproject.toml` and is fetched automatically. `just` itself is packaged for most systems, listed under [Packages](https://github.com/casey/just#packages).

`just test-rust` links `libpython`, so on Debian and Ubuntu the development package of the interpreter `cargo` picks up has to be present, or the build stops at `rust-lld: error: unable to find library -lpython3.14`:

```sh
sudo apt install libpython3.14-dev
```

## Before opening a pull request

Run `just ci`: the checks CI gates on, on your own platform. CI also runs two that it leaves out:

- The release builds, which build and check the wheels for every platform the [Install](README.md#install) section lists.
- `Fixtures`, which rebuilds `tests/data` inside a pinned image and fails on any difference. It needs Docker; run `just regenerate-check` when a change touches the crate or `tests/data`, and `just regenerate` to rewrite what it reports.

`just install` also wires the same recipes into `git commit` through [`pre-commit`](https://pre-commit.com), each one scoped to the files it gates: a change to a test fixture runs both suites, a change to the notices runs the licence check alone. CI scopes its jobs from the same sets: `.github/path-filters.yaml`.

When a change alters something observable (a signature, an error message, a frame count), paste the output before and after it into the pull request.

## Commits

One logical change per commit, so each one can be reverted on its own. The message is one line in the [Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/) shape, with a scope and code in backticks:

```text
fix(resample): trim the startup delay as `rubato` `5.0.1` does
test(opus): pin a cut-short `OpusHead`, zero channels, a long pre-skip and a negative gain
```
