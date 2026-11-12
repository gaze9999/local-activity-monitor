use std::ops::Range;

use pyo3::prelude::*;
use pyo3::types::PyBytes;

mod records;

#[pyclass]
struct Rows {
    source: Py<PyBytes>,
    remaining: Range<usize>,
    limit: usize,
    reverse: bool,
}

#[pymethods]
impl Rows {
    fn __iter__(this: PyRef<'_, Self>) -> PyRef<'_, Self> {
        this
    }

    fn __next__<'py>(&mut self, py: Python<'py>) -> Option<(usize, Bound<'py, PyBytes>)> {
        let data = self.source.bind(py).as_bytes();
        let (offset, line) =
            lam_session_core::next_line(data, &mut self.remaining, self.limit, self.reverse)?;
        Some((offset, PyBytes::new(py, line)))
    }
}

#[pyfunction]
fn forward<'py>(
    py: Python<'py>,
    source: &Bound<'py, PyBytes>,
    discard: bool,
    limit: usize,
) -> (usize, Bound<'py, PyBytes>, bool, Rows) {
    let data = source.as_bytes();
    let batch = lam_session_core::forward(data, discard, limit);
    let rows = Rows {
        source: source.clone().unbind(),
        remaining: batch.lines,
        limit,
        reverse: false,
    };
    (
        batch.skipped,
        PyBytes::new(py, &data[batch.tail]),
        batch.discard,
        rows,
    )
}

#[pyfunction]
fn backfill(
    source: &Bound<'_, PyBytes>,
    skip_prefix: bool,
    limit: usize,
    reverse: bool,
) -> (usize, Rows) {
    let data = source.as_bytes();
    let start = if skip_prefix {
        lam_session_core::prefix_width(data)
    } else {
        0
    };
    (
        start,
        Rows {
            source: source.clone().unbind(),
            remaining: start..data.len(),
            limit,
            reverse,
        },
    )
}

#[pymodule]
fn _session_scan(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_function(wrap_pyfunction!(forward, module)?)?;
    module.add_function(wrap_pyfunction!(backfill, module)?)?;
    module.add_function(wrap_pyfunction!(records::decode, module)?)?;
    module.add_function(wrap_pyfunction!(records::counters, module)?)?;
    Ok(())
}
