use pyo3::prelude::*;
use pyo3::types::PyBytes;

#[pyclass]
struct Rows {
    source: Py<PyBytes>,
    start: usize,
    end: usize,
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
        while self.start < self.end {
            let chunk = &data[self.start..self.end];
            let (offset, line) = if self.reverse {
                let width = chunk
                    .iter()
                    .rposition(|byte| *byte == b'\n')
                    .map_or(0, |index| index + 1);
                let offset = self.start + width;
                self.end = self.start + width.saturating_sub(1);
                (offset, &chunk[width..])
            } else {
                let offset = self.start;
                let width = chunk
                    .iter()
                    .position(|byte| *byte == b'\n')
                    .unwrap_or(chunk.len());
                self.start += width + 1;
                (offset, &chunk[..width])
            };
            if !line.is_empty() && line.len() <= self.limit {
                return Some((offset, PyBytes::new(py, line)));
            }
        }
        None
    }
}

#[pyfunction]
fn forward<'py>(
    py: Python<'py>,
    source: &Bound<'py, PyBytes>,
    mut discard: bool,
    limit: usize,
) -> (usize, Bound<'py, PyBytes>, bool, Rows) {
    let data = source.as_bytes();
    let end = data
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map_or(0, |index| index + 1);
    let skipped = if discard && end > 0 {
        discard = false;
        prefix_width(data)
    } else {
        0
    };
    let mut tail = &data[end..];
    if tail.len() > limit {
        tail = &[];
        discard = true;
    }
    let rows = Rows {
        source: source.clone().unbind(),
        start: skipped,
        end,
        limit,
        reverse: false,
    };
    (skipped, PyBytes::new(py, tail), discard, rows)
}

// An unterminated first fragment has a virtual LF for the backfill cursor.
fn prefix_width(data: &[u8]) -> usize {
    data.iter()
        .position(|byte| *byte == b'\n')
        .unwrap_or(data.len())
        + 1
}

#[pyfunction]
fn backfill(
    source: &Bound<'_, PyBytes>,
    skip_prefix: bool,
    limit: usize,
    reverse: bool,
) -> (usize, Rows) {
    let data = source.as_bytes();
    let start = if skip_prefix { prefix_width(data) } else { 0 };
    (
        start,
        Rows {
            source: source.clone().unbind(),
            start,
            end: data.len(),
            limit,
            reverse,
        },
    )
}

#[pymodule]
fn _session_scan(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_function(wrap_pyfunction!(forward, module)?)?;
    module.add_function(wrap_pyfunction!(backfill, module)?)?;
    Ok(())
}
