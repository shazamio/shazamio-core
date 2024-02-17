use pyo3::exceptions::PyException;
use pyo3::types::PyString;
use pyo3::{pyclass, pymethods, PyErrArguments, PyObject, PyResult, Python, ToPyObject};

trait BaseException {
    fn __str__(&self) -> PyResult<String>;
    fn __repr__(&self) -> PyResult<String>;
}

#[pyclass(extends = PyException)]
pub struct SignatureError {
    message: String,
}

#[pymethods]
impl SignatureError {
    #[new]
    pub fn new(message: String) -> Self {
        SignatureError { message }
    }
}

impl BaseException for SignatureError {
    fn __str__(&self) -> PyResult<String> {
        Ok(self.message.to_string())
    }

    fn __repr__(&self) -> PyResult<String> {
        Ok(format!("SignatureError({message})", message = self.message))
    }
}

impl PyErrArguments for SignatureError {
    fn arguments(self, py: Python) -> PyObject {
        PyString::new(py, &self.message).to_object(py)
    }
}
