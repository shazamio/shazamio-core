use crate::errors::SignatureError;
use crate::fingerprinting::communication;
use crate::fingerprinting::communication::get_signature_json;
use crate::fingerprinting::signature_format::DecodedSignature;
use crate::response::{Geolocation, Signature, SignatureSong};
use pyo3::{Bound, IntoPyObject, PyAny, PyErr, PyResult, Python};
use std::future::Future;
use tokio::task;

pub fn get_python_future<'py, T>(
    py: Python<'py>,
    future: impl Future<Output = PyResult<T>> + Send + 'static,
) -> PyResult<Bound<'py, PyAny>>
where
    T: for<'a> IntoPyObject<'a> + Send + 'static,
{
    return pyo3_async_runtimes::tokio::future_into_py(py, async move {
        task::spawn_blocking(move || futures::executor::block_on(future))
            .await
            .unwrap()
    });
}

pub fn convert_signature_to_py(signature: communication::Signature) -> PyResult<Signature> {
    Signature::new(
        Geolocation::new(
            signature.geolocation.altitude,
            signature.geolocation.latitude,
            signature.geolocation.longitude,
        )?,
        SignatureSong::new(
            signature.signature.samples,
            signature.signature.timestamp,
            signature.signature.uri,
        )?,
        signature.timestamp,
        signature.timezone,
    )
}

pub fn unwrap_decoded_signature(data: DecodedSignature) -> Result<communication::Signature, PyErr> {
    get_signature_json(&data).map_err(|e| {
        let error_message = format!("{}", e);
        PyErr::new::<SignatureError, _>(error_message)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    // `get_python_future` is not here: it needs a running `asyncio` loop, which is
    //  what the Python suite already gives it.

    // The conversion maps its fields positionally, so every one of them is pinned
    //  rather than a sample: a swapped `latitude`/`longitude` type-checks.
    #[test]
    fn the_response_carries_every_field_of_the_signature_it_converts() {
        let decoded = DecodedSignature {
            sample_rate_hz: 16000,
            number_samples: 24_500,
            frequency_band_to_sound_peaks: HashMap::new(),
        };

        let signature = unwrap_decoded_signature(decoded).unwrap();

        let altitude = signature.geolocation.altitude;
        let latitude = signature.geolocation.latitude;
        let longitude = signature.geolocation.longitude;

        let samples = signature.signature.samples;
        let timestamp = signature.timestamp;
        let timezone = signature.timezone.clone();
        let uri = signature.signature.uri.clone();

        let converted = convert_signature_to_py(signature).unwrap();

        assert_eq!(converted.geolocation.altitude, altitude);
        assert_eq!(converted.geolocation.latitude, latitude);
        assert_eq!(converted.geolocation.longitude, longitude);
        assert_eq!(converted.signature.samples, samples);
        assert_eq!(converted.signature.uri, uri);
        assert_eq!(converted.timestamp, timestamp);
        assert_eq!(converted.timezone, timezone);
    }
}
