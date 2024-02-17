use pyo3::{pyclass, pymethods, PyResult};
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
#[pyclass]
pub(crate) struct Geolocation {
    #[pyo3(get)]
    pub(crate) altitude: i16,
    #[pyo3(get)]
    pub(crate) latitude: i8,
    #[pyo3(get)]
    pub(crate) longitude: i8,
}

#[derive(Clone, Serialize, Deserialize)]
#[pyclass]
pub(crate) struct SignatureSong {
    #[pyo3(get)]
    pub(crate) samples: u32,
    #[pyo3(get)]
    pub(crate) timestamp: u32,
    #[pyo3(get)]
    pub(crate) uri: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[pyclass]
pub(crate) struct Signature {
    #[pyo3(get)]
    pub(crate) geolocation: Geolocation,
    #[pyo3(get)]
    pub(crate) signature: SignatureSong,
    #[pyo3(get)]
    pub(crate) timestamp: u32,
    #[pyo3(get)]
    pub(crate) timezone: String,
}

#[pymethods]
impl Geolocation {
    #[new]
    pub fn new(altitude: i16, latitude: i8, longitude: i8) -> PyResult<Self> {
        Ok(Geolocation {
            altitude,
            latitude,
            longitude,
        })
    }
}

#[pymethods]
impl SignatureSong {
    #[new]
    pub fn new(samples: u32, timestamp: u32, uri: String) -> PyResult<Self> {
        Ok(SignatureSong {
            samples,
            timestamp,
            uri,
        })
    }
}

#[pymethods]
impl Signature {
    #[new]
    pub fn new(
        geolocation: Geolocation,
        signature: SignatureSong,
        timestamp: u32,
        timezone: String,
    ) -> PyResult<Self> {
        Ok(Signature {
            geolocation,
            signature,
            timestamp,
            timezone,
        })
    }
}
