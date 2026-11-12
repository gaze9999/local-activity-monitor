use jiter::JsonValue;
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyDict, PyInt, PyList, PyString, PyTuple};
use pyo3::IntoPyObjectExt;

#[pyfunction]
pub fn decode<'py>(py: Python<'py>, source: &Bound<'py, PyAny>) -> PyResult<Bound<'py, PyAny>> {
    let data = if let Ok(bytes) = source.cast::<PyBytes>() {
        Some(bytes.as_bytes())
    } else {
        source
            .cast::<PyString>()
            .ok()
            .and_then(|text| text.to_str().ok())
            .map(str::as_bytes)
    };
    match data.and_then(|data| JsonValue::parse(data, false).ok()) {
        Some(value) => python_value(py, &value),
        // CPython also accepts formats such as BOM-marked text and wide integers.
        None => Ok(py.NotImplemented().into_bound(py)),
    }
}

fn python_value<'py>(py: Python<'py>, value: &JsonValue<'_>) -> PyResult<Bound<'py, PyAny>> {
    match value {
        JsonValue::Null => Ok(py.None().into_bound(py)),
        JsonValue::Bool(value) => value.into_bound_py_any(py),
        JsonValue::Int(value) => value.into_bound_py_any(py),
        JsonValue::Float(value) => value.into_bound_py_any(py),
        JsonValue::Str(value) => value.as_ref().into_bound_py_any(py),
        JsonValue::Array(values) => {
            let items = PyList::empty(py);
            for value in values.iter() {
                items.append(python_value(py, value)?)?;
            }
            Ok(items.into_any())
        }
        JsonValue::Object(values) => {
            let object = PyDict::new(py);
            for (key, value) in values.iter() {
                object.set_item(key.as_ref(), python_value(py, value)?)?;
            }
            Ok(object.into_any())
        }
    }
}

#[pyfunction]
pub fn counters<'py>(
    py: Python<'py>,
    usage: &Bound<'py, PyDict>,
    keys: &Bound<'py, PyTuple>,
) -> PyResult<Option<Bound<'py, PyDict>>> {
    let clean = PyDict::new(py);
    let mut known = false;
    for key in keys.iter() {
        let value = usage
            .get_item(&key)?
            .filter(|value| value.is_exact_instance_of::<PyInt>())
            .and_then(|value| value.extract::<i64>().ok())
            .filter(|value| *value >= 0);
        known |= value.is_some();
        clean.set_item(&key, value)?;
    }
    Ok(known.then_some(clean))
}
