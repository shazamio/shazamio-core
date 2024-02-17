use crate::errors::SignatureError;
use crate::fingerprinting::communication;
use crate::fingerprinting::communication::get_signature_json;
use crate::fingerprinting::signature_format::DecodedSignature;
use crate::response::{Geolocation, Signature, SignatureSong};
use pyo3::{IntoPy, Py, PyAny, PyErr, PyResult, Python};
use std::future::Future;
use tokio::task;

pub fn get_python_future<'py, T>(
    py: Python<'py>,
    future: impl Future<Output=PyResult<T>> + Send + 'static,
) -> PyResult<&'py PyAny>
    where
        T: Send + 'static,
        T: IntoPy<Py<PyAny>>,
{
    return pyo3_asyncio::tokio::future_into_py(py, async move {
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
        PyErr::new::<SignatureError, _>(SignatureError::new(error_message))
    })
}
