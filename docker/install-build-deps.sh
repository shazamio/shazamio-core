#!/usr/bin/env bash

set -e

# The one thing the two base images do not share. `clang` comes from EPEL and RPM
#  Fusion on the glibc image; the musl image is Alpine based and already carries the
#  compiler, CMake and `patchelf` this build uses, so nothing is installed there.
if command -v dnf > /dev/null; then
    dnf install -y epel-release
    dnf install -y --nogpgcheck https://download1.rpmfusion.org/free/el/rpmfusion-free-release-8.noarch.rpm
    dnf install -y --nogpgcheck https://download1.rpmfusion.org/nonfree/el/rpmfusion-nonfree-release-8.noarch.rpm
    dnf install -y clang clang-devel
fi

curl -o rustup.sh --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs
sh rustup.sh -y
source "$HOME/.cargo/env"
rustup update
rustc -V

cargo install cargo-chef --locked
