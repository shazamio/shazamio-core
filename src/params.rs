use crate::fingerprinting::algorithm::DEFAULT_SEGMENT_DURATION_SECONDS;
use pyo3::exceptions::PyValueError;
use pyo3::{pyclass, pymethods, PyResult};
use serde::{Deserialize, Serialize};

// A zero-second segment fingerprints nothing: it produced an empty signature and
//  reported success. There is no upper bound to enforce, because a duration at or
//  above the file length uses the whole file.
pub(crate) fn validated_segment_duration_seconds(value: u32) -> PyResult<u32> {
    if value == 0 {
        return Err(PyValueError::new_err(
            "segment_duration_seconds must be at least 1",
        ));
    }

    Ok(value)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[pyclass(from_py_object, module = "shazamio_core")]
pub(crate) struct SearchParams {
    pub(crate) segment_duration_seconds: u32,
}
#[pymethods]
impl SearchParams {
    #[new]
    #[pyo3(signature = (segment_duration_seconds=None))]
    pub fn new(segment_duration_seconds: Option<u32>) -> PyResult<Self> {
        Ok(SearchParams {
            segment_duration_seconds: validated_segment_duration_seconds(
                segment_duration_seconds.unwrap_or(DEFAULT_SEGMENT_DURATION_SECONDS),
            )?,
        })
    }

    #[getter]
    fn get_segment_duration_seconds(&self) -> u32 {
        self.segment_duration_seconds
    }

    #[setter]
    fn set_segment_duration_seconds(&mut self, value: u32) -> PyResult<()> {
        self.segment_duration_seconds = validated_segment_duration_seconds(value)?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_segment_duration_defaults_to_ten_seconds() {
        assert_eq!(
            SearchParams::new(None).unwrap().segment_duration_seconds,
            10
        );
        assert_eq!(
            SearchParams::new(Some(4)).unwrap().segment_duration_seconds,
            4
        );
    }

    #[test]
    fn a_zero_segment_duration_is_refused() {
        let mut parameters = SearchParams::new(Some(4)).unwrap();

        assert!(SearchParams::new(Some(0)).is_err());
        assert!(parameters.set_segment_duration_seconds(0).is_err());
        assert_eq!(parameters.segment_duration_seconds, 4);
    }
}
