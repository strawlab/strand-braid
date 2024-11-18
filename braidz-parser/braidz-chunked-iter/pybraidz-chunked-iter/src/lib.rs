// Copyright 2023 Andrew D. Straw.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT
// or http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

use numpy::convert::IntoPyArray;
use pyo3::{exceptions::PyValueError, prelude::*, types::PyDict};

use braidz_chunked_iter::{ChunkSize, DurationChunk, ToChunkIter};
use csv_eof::EarlyEofOk;
use zip_or_dir::{MaybeGzReader, ZipDirArchive};

macro_rules! dict_set_item_array {
    ($dict:expr, $name:expr, $obj:expr, $py: expr) => {
        if $dict.set_item($name, $obj.into_pyarray_bound($py)).is_err() {
            panic!("error while setting '{}' key on data_dict", $name);
        }
    };
}

#[pyclass(unsendable)]
struct KalmanEstimatesChunker {
    chunker: &'static mut dyn Iterator<Item = DurationChunk>,
}

impl KalmanEstimatesChunker {
    fn new(path: &str, sz: ChunkSize) -> PyResult<Self> {
        let archive = zip_or_dir::ZipDirArchive::auto_from_path(path).map_err(|e| {
            PyErr::new::<PyValueError, _>(format!("Could not open file {}: '{}'", path, e))
        })?;
        // leak to get static lifetime
        let archive: &'static mut ZipDirArchive<_> = Box::leak(Box::new(archive));

        let mut first_row = None;
        let src_fname = flydra_types::KALMAN_ESTIMATES_CSV_FNAME;

        {
            let rdr = archive.open_raw_or_gz(src_fname).map_err(|e| {
                PyErr::new::<PyValueError, _>(format!(
                    "Could not open file '{src_fname}' in archive '{path}': '{e}'"
                ))
            })?;
            let kest_reader = csv::Reader::from_reader(rdr);

            if let Some(row) = kest_reader.into_deserialize().early_eof_ok().next() {
                let row = row.map_err(|e| {
                    PyErr::new::<PyValueError, _>(format!("Error reading row: '{e}'"))
                })?;
                first_row = Some(row);
            }
        }
        if let Some(first_row) = first_row {
            let rdr = archive.open_raw_or_gz(src_fname).map_err(|e| {
                PyErr::new::<PyValueError, _>(format!(
                    "Could not open file '{src_fname}' in archive '{path}': '{e}'"
                ))
            })?;
            let t1: csv::Reader<MaybeGzReader<'_>> = csv::Reader::from_reader(rdr);

            let inner_iter = t1.into_deserialize().early_eof_ok();
            let my_iter = ToChunkIter::to_chunk_iter(inner_iter, first_row, sz).map_err(|e| {
                PyErr::new::<PyValueError, _>(format!("Could chunk based on duration: '{e}'"))
            })?;
            let chunker = Box::new(my_iter);
            // leak to get static lifetime
            let chunker = Box::leak(chunker);
            Ok(KalmanEstimatesChunker { chunker })
        } else {
            Err(PyErr::new::<PyValueError, _>(format!(
                "no rows in {src_fname}"
            )))
        }
    }
}

#[pymethods]
impl KalmanEstimatesChunker {
    fn __iter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    fn __next__(mut slf: PyRefMut<'_, Self>) -> Option<PyObject> {
        let chunk = match slf.chunker.next() {
            Some(chunk) => chunk,
            None => {
                return None;
            }
        };
        let n_rows = chunk.rows.len();
        let result_dict = PyDict::new_bound(slf.py());
        if result_dict.set_item("n_rows", n_rows).is_err() {
            panic!("error while setting 'n_rows' key on result_dict");
        }
        let data_dict = PyDict::new_bound(slf.py());

        let mut obj_id = Vec::with_capacity(n_rows);
        let mut frame = Vec::with_capacity(n_rows);
        let mut timestamp = Vec::with_capacity(n_rows);
        let mut x = Vec::with_capacity(n_rows);
        let mut y = Vec::with_capacity(n_rows);
        let mut z = Vec::with_capacity(n_rows);
        let mut xvel = Vec::with_capacity(n_rows);
        let mut yvel = Vec::with_capacity(n_rows);
        let mut zvel = Vec::with_capacity(n_rows);
        for row in chunk.rows.into_iter() {
            let ts = match row.timestamp {
                Some(ref tl) => tl.as_f64(),
                None => f64::NAN,
            };
            obj_id.push(row.obj_id);
            frame.push(row.frame.0);
            timestamp.push(ts);
            x.push(row.x);
            y.push(row.y);
            z.push(row.z);
            xvel.push(row.xvel);
            yvel.push(row.yvel);
            zvel.push(row.zvel);
        }

        dict_set_item_array!(data_dict, "obj_id", obj_id, slf.py());
        dict_set_item_array!(data_dict, "frame", frame, slf.py());
        dict_set_item_array!(data_dict, "timestamp", timestamp, slf.py());
        dict_set_item_array!(data_dict, "x", x, slf.py());
        dict_set_item_array!(data_dict, "y", y, slf.py());
        dict_set_item_array!(data_dict, "z", z, slf.py());
        dict_set_item_array!(data_dict, "xvel", xvel, slf.py());
        dict_set_item_array!(data_dict, "yvel", yvel, slf.py());
        dict_set_item_array!(data_dict, "zvel", zvel, slf.py());

        if result_dict.set_item("data", data_dict).is_err() {
            panic!("error while setting 'data_dict' key on result_dict");
        }

        Some(result_dict.into())
    }
}

/// Iterate over duration-defined chunks of the `kalman_estimates` table.
///
/// Parameters
/// ----------
/// path : str
///     The path of the `.braidz` file (or `.braid` directory) to open.
/// duration_seconds: float
///     The duration of each chunk, in seconds.
#[pyfunction]
fn chunk_on_duration(path: &str, duration_seconds: f64) -> PyResult<KalmanEstimatesChunker> {
    let chunk_dur = std::time::Duration::from_secs_f64(duration_seconds);
    let sz = ChunkSize::TimestampDuration(chunk_dur);
    KalmanEstimatesChunker::new(path, sz)
}

/// Iterate over duration-defined chunks of the `kalman_estimates` table.
///
/// Parameters
/// ----------
/// path : str
///     The path of the `.braidz` file (or `.braid` directory) to open.
/// num_frames: int
///     The number of frames included in each chunk.
#[pyfunction]
fn chunk_on_num_frames(path: &str, num_frames: usize) -> PyResult<KalmanEstimatesChunker> {
    let sz = ChunkSize::FrameNumber(num_frames);
    KalmanEstimatesChunker::new(path, sz)
}

/// Chunked iteration over tables in `.braidz` files.
#[pymodule]
fn pybraidz_chunked_iter(_py: Python, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<KalmanEstimatesChunker>()?;
    m.add_function(wrap_pyfunction!(chunk_on_duration, m)?)?;
    m.add_function(wrap_pyfunction!(chunk_on_num_frames, m)?)?;
    Ok(())
}
