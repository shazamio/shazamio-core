use pyo3::create_exception;
use pyo3::exceptions::PyException;

// `#[pyclass(extends = PyException)]` cannot be used here: subclassing a native
// type needs its C struct layout, which the limited API this crate now builds
// against does not expose, and `cargo check` rejects it outright:
//   error[E0277]: pyclass `PyException` cannot be subclassed
// `create_exception!` builds the same class through `PyErr_NewException`, which
// is part of the limited API.
create_exception!(shazamio_core, SignatureError, PyException);
