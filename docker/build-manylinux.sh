#!/usr/bin/env bash

set -e

if [[ -n $PYTHON_INTERPRETER ]]; then
    maturin build -i "$PYTHON_INTERPRETER" --release --out dist
    exit 0
fi

maturin build --release --out dist

# `abi3` covers only CPython. PyPy has no stable ABI, so it is left out of that
# wheel and `maturin` skips it unless the interpreter is named -- without this
# the image stops producing the `pp*` wheel it publishes today. `pyo3` turns
# `abi3` off by itself on PyPy, so naming the interpreter is the whole fix.
shopt -s nullglob
for pypy_interpreter in /opt/python/pp*/bin/python; do
    maturin build -i "$pypy_interpreter" --release --out dist
done
