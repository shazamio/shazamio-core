#!/usr/bin/env bash

set -e

maturin build --release --out dist

# `abi3` covers only CPython. PyPy has no stable ABI, so it is left out of that
#  wheel and `maturin` skips it unless the interpreter is named: without this the
#  image stops producing the `pp*` wheels it publishes today. `pyo3` turns `abi3` off
#  by itself on PyPy, so naming the interpreter is the whole fix. The musl image
#  ships no PyPy at all, which is what `nullglob` covers.
shopt -s nullglob

# A free-threaded CPython cannot load `abi3` either; which wheel each gets is argued
#  above `pyo3` in `Cargo.toml`. Named, not globbed, so Linux ships the set the other
#  platforms build and fails on an image that drops one; keep in sync with `ci.yaml`.
for free_threaded_interpreter in /opt/python/cp314-cp314t/bin/python /opt/python/cp315-cp315t/bin/python; do
    maturin build -i "$free_threaded_interpreter" --release --out dist
done

for pypy_interpreter in /opt/python/pp*/bin/python; do
    maturin build -i "$pypy_interpreter" --release --out dist
done
