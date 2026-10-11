use pyo3::pyclass;

#[derive(Clone)]
#[pyclass(skip_from_py_object, module = "shazamio_core")]
pub(crate) struct SignatureSong {
    #[pyo3(get)]
    pub(crate) samples: u32,
    #[pyo3(get)]
    pub(crate) uri: String,
}

#[pyclass(module = "shazamio_core")]
pub(crate) struct Signature {
    #[pyo3(get)]
    pub(crate) signature: SignatureSong,
    #[pyo3(get)]
    pub(crate) timestamp: u64,
    #[pyo3(get)]
    pub(crate) timezone: String,
}
