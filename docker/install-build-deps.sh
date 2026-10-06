#!/usr/bin/env bash

set -e

curl -o rustup.sh --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs
sh rustup.sh -y
# The file is written by the `rustup` install above, so it exists only inside the
#  image and `shellcheck` cannot follow it.
# shellcheck source=/dev/null
source "$HOME/.cargo/env"
rustup update
rustc -V

cargo install cargo-chef --locked
