use pyo3::{pyclass, pymethods};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[pyclass(from_py_object)]
pub(crate) struct SearchParams {
    #[pyo3(get, set)]
    pub(crate) segment_duration_seconds: u32,
}
#[pymethods]
impl SearchParams {
    #[new]
    #[pyo3(signature = (segment_duration_seconds=None))]
    pub fn new(segment_duration_seconds: Option<u32>) -> Self {
        SearchParams {
            segment_duration_seconds: segment_duration_seconds.unwrap_or(10),
        }
    }
}