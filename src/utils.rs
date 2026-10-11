use crate::errors::SignatureError;
use crate::fingerprinting::communication::get_signature_json;
use crate::fingerprinting::signature_format::DecodedSignature;
use crate::response::Signature;
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

pub fn unwrap_decoded_signature(data: DecodedSignature) -> Result<Signature, PyErr> {
    get_signature_json(&data).map_err(|e| {
        let error_message = format!("{}", e);
        PyErr::new::<SignatureError, _>(error_message)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use pyo3::Py;

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
}
