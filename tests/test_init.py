import sys

import pytest

import shazamio_core
from shazamio_core import shazamio_core as extension


def test_package_exports_everything_the_extension_declares() -> None:
    """`py.typed` makes a plain `from .shazamio_core import Name` export nothing.

    Under the PEP 484 re-export rules the name is importable at runtime and is a
    `reportPrivateImportUsage` error under `pyright` for everyone who follows the
    README. `__all__` in `__init__.py` is what makes the six names public.
    https://typing.python.org/en/latest/spec/distributing.html#import-conventions
    """
    # `pyo3` appends every name to the module's `__all__` as it is added, so the
    #  extension's list is the whole surface the Rust side declares.
    #  https://github.com/PyO3/pyo3/blob/v0.29.2/src/types/module.rs#L497-L502
    assert sorted(shazamio_core.__all__) == sorted(extension.__all__)


@pytest.mark.parametrize("name", sorted(shazamio_core.__all__))
def test_every_public_name_reports_the_package_as_its_module(name: str) -> None:
    """`__module__` is an import path, and `repr` and `pickle` both read it as one.

    `pyo3` builds `tp_name` from the `module` argument of `#[pyclass]` and falls back
    to `builtins`, which every class here used to inherit, so `repr()` read
    `<builtins.Geolocation object at 0x...>` and `pickle` could not name the class.
    https://github.com/PyO3/pyo3/blob/v0.29.2/src/pyclass/create_type_object.rs#L610-L616
    """
    declared = getattr(shazamio_core, name)

    assert declared.__module__ == shazamio_core.__name__

    # What the attribute is for: the path has to lead back to this exact object, which
    #  is how `pickle` resolves a class it has only the name of. Pickling also needs a
    #  `__getnewargs__`, which nothing here declares yet.
    assert getattr(sys.modules[declared.__module__], declared.__qualname__) is declared
