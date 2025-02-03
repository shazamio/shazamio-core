mod errors;
mod fingerprinting;
mod response;
mod utils;
mod params;

use crate::errors::SignatureError;
use crate::response::{Geolocation, Signature, SignatureSong};
use crate::params::SearchParams;
use crate::utils::convert_signature_to_py;
use crate::utils::get_python_future;
use crate::utils::unwrap_decoded_signature;
use fingerprinting::algorithm::SignatureGenerator;
use pyo3::prelude::*;
use pyo3::{pyclass, pymethods, pymodule, PyErr, PyObject, PyResult, Python, ToPyObject};
use log::{info, debug, error};

#[pymodule]
fn shazamio_core(_py: Python<'_>, m: &PyModule) -> PyResult<()> {
    pyo3_log::init();
    info!("Initializing shazamio_core module");

    m.add_class::<Recognizer>()?;
    m.add_class::<SignatureError>()?;
    m.add_class::<Geolocation>()?;
    m.add_class::<SignatureSong>()?;
    m.add_class::<Signature>()?;
    m.add_class::<SearchParams>()?;

    info!("shazamio_core module initialized successfully");
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
        let duration = segment_duration_seconds.unwrap_or(10);
        info!("Recognizer created with segment_duration_seconds = {}", duration);
        Recognizer { segment_duration_seconds: duration }
    }

    fn recognize_bytes(
        &self,
        py: Python,
        bytes: Vec<u8>,
        options: Option<SearchParams>,
    ) -> PyResult<PyObject> {
        debug!(
            "recognize_bytes method called with bytes len: {} and options: {:?}",
            bytes.len(),
            options,
        );

        let search_options = options.unwrap_or_else(|| {
            debug!(
                "Options not provided, using default segment duration {}",
                self.segment_duration_seconds,
            );
            SearchParams::new(Option::from(self.segment_duration_seconds))
        });

        let future = async move {
            debug!("Starting async recognition from bytes");
            let data = SignatureGenerator::make_signature_from_bytes(
                bytes,
                Some(search_options.segment_duration_seconds),
            ).map_err(|e| {
                error!("Error in make_signature_from_bytes: {}", e);
                let error_message = format!("{}", e);
                PyErr::new::<SignatureError, _>(SignatureError::new(error_message))
            })?;

            debug!("Successfully generated signature from bytes");
            let signature = unwrap_decoded_signature(data);
            convert_signature_to_py(signature?)
        };

        let python_future = get_python_future(py, future);
        debug!("Returning Python future for recognize_bytes");
        python_future.map(|any| any.to_object(py))
    }

    fn recognize_path(
        &self,
        py: Python,
        value: String,
        options: Option<SearchParams>,
    ) -> PyResult<PyObject> {
        debug!(
            "recognize_path method called with path: {} and options: {:?}",
            value,
            options,
        );

        let search_options = options.unwrap_or_else(|| {
            debug!(
                "Options not provided, using default segment duration {}",
                self.segment_duration_seconds,
            );
            SearchParams::new(Option::from(self.segment_duration_seconds))
        });

        let future = async move {
            debug!("Starting async recognition from file: {}", value);
            let data = SignatureGenerator::make_signature_from_file(
                &value,
                Some(search_options.segment_duration_seconds),
            ).map_err(|e| {
                debug!("Error in make_signature_from_file: {}", e);
                let error_message = format!("{}", e);
                PyErr::new::<SignatureError, _>(SignatureError::new(error_message))
            })?;

            debug!("Successfully generated signature from file");
            let signature = unwrap_decoded_signature(data);
            convert_signature_to_py(signature?)
        };

        let python_future = get_python_future(py, future);
        debug!("Returning Python future for recognize_path");
        python_future.map(|any| any.to_object(py))
    }
}
