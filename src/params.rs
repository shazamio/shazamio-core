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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_segment_duration_defaults_to_ten_seconds() {
        assert_eq!(SearchParams::new(None).segment_duration_seconds, 10);
        assert_eq!(SearchParams::new(Some(4)).segment_duration_seconds, 4);
    }
}
