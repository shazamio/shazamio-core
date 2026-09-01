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
