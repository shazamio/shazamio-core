use crate::fingerprinting::algorithm::DEFAULT_SEGMENT_SECONDS;
use pyo3::exceptions::{PyOverflowError, PyValueError};
use pyo3::{pyclass, pymethods, Borrowed, FromPyObject, PyAny, PyErr, PyResult};

const OUT_OF_RANGE: &str = "segment_duration_seconds must be at least 1 and at most 4294967295";

/// A segment duration in whole seconds, refused with `ValueError` outside `1..=u32::MAX`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SegmentSeconds(pub(crate) u32);

impl SegmentSeconds {
    pub(crate) const DEFAULT: Self = Self(DEFAULT_SEGMENT_SECONDS);

    // A zero-second segment fingerprints nothing: it produced an empty signature and
    //  reported success. A duration at or above the file length uses the whole file,
    //  so the only upper bound is the type's.
    pub(crate) fn new(value: u32) -> PyResult<Self> {
        if value == 0 {
            return Err(PyValueError::new_err(OUT_OF_RANGE));
        }

        Ok(Self(value))
    }
}

impl<'a, 'py> FromPyObject<'a, 'py> for SegmentSeconds {
    type Error = PyErr;

    // Extracting a `u32` refuses a negative or oversized int with `OverflowError`,
    //  which a caller catching `ValueError` for bad input misses.
    //  https://github.com/PyO3/pyo3/blob/451d99fdcdcddf159e8e0a1186960b332cdb5d7c/src/conversions/std/num.rs#L175-L180
    fn extract(obj: Borrowed<'a, 'py, PyAny>) -> PyResult<Self> {
        match obj.extract::<u32>() {
            Ok(value) => Self::new(value),
            Err(error) if error.is_instance_of::<PyOverflowError>(obj.py()) => {
                Err(PyValueError::new_err(OUT_OF_RANGE))
            }
            Err(error) => Err(error),
        }
    }
}

#[derive(Debug, Clone)]
#[pyclass(from_py_object, module = "shazamio_core")]
pub(crate) struct SearchParams {
    pub(crate) segment_duration_seconds: u32,
}
#[pymethods]
impl SearchParams {
    #[new]
    #[pyo3(signature = (segment_duration_seconds=None))]
    pub fn new(segment_duration_seconds: Option<SegmentSeconds>) -> Self {
        SearchParams {
            segment_duration_seconds: segment_duration_seconds
                .unwrap_or(SegmentSeconds::DEFAULT)
                .0,
        }
    }

    #[getter]
    fn get_segment_duration_seconds(&self) -> u32 {
        self.segment_duration_seconds
    }

    #[setter]
    fn set_segment_duration_seconds(&mut self, value: SegmentSeconds) {
        self.segment_duration_seconds = value.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_segment_duration_defaults_to_twelve_seconds() {
        assert_eq!(SearchParams::new(None).segment_duration_seconds, 12);
        assert_eq!(
            SearchParams::new(Some(SegmentSeconds(4))).segment_duration_seconds,
            4
        );
    }

    #[test]
    fn a_zero_segment_duration_is_refused() {
        assert!(SegmentSeconds::new(0).is_err());
        assert_eq!(SegmentSeconds::new(1).unwrap(), SegmentSeconds(1));
    }
}
