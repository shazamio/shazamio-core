use crate::errors::SignatureError;
use crate::fingerprinting::communication;
use crate::fingerprinting::communication::get_signature_json;
use crate::fingerprinting::signature_format::DecodedSignature;
use crate::response::{Geolocation, Signature, SignatureSong};
use pyo3::{Bound, IntoPyObject, PyAny, PyErr, PyResult, Python};
use pyo3_async_runtimes::err::RustPanic;
use std::any::Any;
use std::future::Future;
use tokio::task;

pub fn get_python_future<'py, T>(
    py: Python<'py>,
    future: impl Future<Output = PyResult<T>> + Send + 'static,
) -> PyResult<Bound<'py, PyAny>>
where
    T: for<'a> IntoPyObject<'a> + Send + 'static,
{
    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        // The panic is raised here rather than left to `future_into_py`, which reports
        //  every one as `rust future panicked: unknown error`: it downcasts a
        //  `&Box<dyn Any>`, so the `&str` and `String` arms never match. Can go once
        //  this is fixed upstream: https://github.com/PyO3/pyo3-async-runtimes/issues/91
        //  https://github.com/PyO3/pyo3-async-runtimes/blob/58d42b7a3eb239719175c5587b2b7debd9ee134b/src/generic.rs#L675
        task::spawn_blocking(move || futures::executor::block_on(future))
            .await
            .unwrap_or_else(|error| {
                let message = match error.try_into_panic() {
                    Ok(payload) => panic_message(&*payload).to_owned(),
                    Err(error) => error.to_string(),
                };
                Err(RustPanic::new_err(format!(
                    "rust future panicked: {message}"
                )))
            })
    })
}

fn panic_message(payload: &(dyn Any + Send)) -> &str {
    if let Some(message) = payload.downcast_ref::<&str>() {
        message
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message
    } else {
        "unknown error"
    }
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
    use pyo3::Py;
    use std::collections::HashMap;

    fn panicking_worker() -> PyResult<impl Future<Output = PyResult<Py<PyAny>>> + Send> {
        Python::attach(|py| {
            let future = get_python_future::<()>(py, async { panic!("worker panic") })?;
            pyo3_async_runtimes::tokio::into_future(future)
        })
    }

    // Through the whole bridge rather than the helper alone: passing `&payload` at
    //  the call site, the upstream mistake, still type-checks and leaves the helper
    //  test green, while Python gets `rust future panicked: unknown error` again.
    #[test]
    fn a_worker_panic_reaches_python_with_its_message() {
        Python::initialize();

        let error = Python::attach(|py| {
            pyo3_async_runtimes::tokio::run(py, async {
                Ok(panicking_worker()?.await.unwrap_err())
            })
        })
        .unwrap();

        Python::attach(|py| {
            assert!(error.is_instance_of::<RustPanic>(py));
            assert_eq!(
                error.value(py).to_string(),
                "rust future panicked: worker panic"
            );
        });
    }

    // Real payloads from `catch_unwind`: a literal panics with `&str`, a formatted
    //  one with `String`, and the box has to be dereferenced before either matches.
    #[test]
    fn a_panic_payload_keeps_its_message() {
        let literal = std::panic::catch_unwind(|| panic!("literal")).unwrap_err();
        let formatted = std::panic::catch_unwind(|| panic!("{}", "formatted")).unwrap_err();
        let other = std::panic::catch_unwind(|| std::panic::panic_any(7)).unwrap_err();

        assert_eq!(panic_message(&*literal), "literal");
        assert_eq!(panic_message(&*formatted), "formatted");
        assert_eq!(panic_message(&*other), "unknown error");
    }

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
