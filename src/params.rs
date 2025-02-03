use pyo3::{pyclass, pymethods};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[pyclass]
pub(crate) struct SearchParams {
    #[pyo3(get, set)]
    pub(crate) segment_duration_seconds: u32,
}
#[pymethods]
impl SearchParams {
    #[new]
    pub fn new(segment_duration_seconds: Option<u32>) -> Self {
        SearchParams {
            segment_duration_seconds: segment_duration_seconds.unwrap_or(10),
        }
    }
}