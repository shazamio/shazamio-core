mod errors;
mod fingerprinting;
mod response;
mod utils;

use crate::errors::SignatureError;
use crate::response::{Geolocation, Signature, SignatureSong};
use crate::utils::convert_signature_to_py;
use crate::utils::get_python_future;
use crate::utils::unwrap_decoded_signature;
use fingerprinting::algorithm::SignatureGenerator;
use pyo3::prelude::PyModule;
use pyo3::{pyclass, pymethods, pymodule, PyErr, PyObject, PyResult, Python, ToPyObject};

#[pymodule]
fn shazamio_core(_py: Python<'_>, m: &PyModule) -> PyResult<()> {
    m.add_class::<Recognizer>()?;
    m.add_class::<SignatureError>()?;
    m.add_class::<Geolocation>()?;
    m.add_class::<SignatureSong>()?;
    m.add_class::<Signature>()?;
    Ok(())
}

#[derive(Clone)]
#[pyclass]
struct Recognizer {
    #[pyo3(get, set)]
    segment_duration_seconds: u32,
}

#[pymethods]
impl Recognizer {
    #[new]
    pub fn new(segment_duration_seconds: Option<u32>) -> Self {
        let duration = segment_duration_seconds.unwrap_or(12);
        Recognizer { segment_duration_seconds: duration }
    }

    fn recognize_bytes(&self, py: Python, bytes: Vec<u8>) -> PyResult<PyObject> {
        let segment_duration = self.segment_duration_seconds;

        let future = async move {
            let data = SignatureGenerator::make_signature_from_bytes(
                bytes,
                Some(segment_duration),
            ).map_err(|e| {
                let error_message = format!("{}", e);
                PyErr::new::<SignatureError, _>(SignatureError::new(error_message))
            })?;

            let signature = unwrap_decoded_signature(data);
            convert_signature_to_py(signature?)
        };

        let python_future = get_python_future(py, future);
        python_future.map(|any| any.to_object(py))
    }

    fn recognize_path(&self, py: Python, value: String) -> PyResult<PyObject> {
        let segment_duration = self.segment_duration_seconds;

        let future = async move {
            let data = SignatureGenerator::make_signature_from_file(
                &value,
                Some(segment_duration),
            ).map_err(|e| {
                let error_message = format!("{}", e);
                PyErr::new::<SignatureError, _>(SignatureError::new(error_message))
            })?;

            let signature = unwrap_decoded_signature(data);
            convert_signature_to_py(signature?)
        };

        let python_future = get_python_future(py, future);
        python_future.map(|any| any.to_object(py))
    }
}