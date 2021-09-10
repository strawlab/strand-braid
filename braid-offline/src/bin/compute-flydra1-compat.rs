// Compute ML_estimates and ML_estimates_2d_idxs tables
use csv_eof::EarlyEofOk;
use itertools::Itertools;
use log::{error, info, trace};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};
use std::io::{Read, Write};
use structopt::StructOpt;

use flydra2::{DataAssocRow, Result};
use groupby::AscendingGroupIter;

use flydra_types::{KalmanEstimatesRow, SyncFno};

use braid_offline::pick_csvgz_or_csv;

// computed later for back-compat
const COMPUTED_DIRNAME: &str = "flydra1-compat-computed-offline";
const ML_ESTIMATES_FNAME: &str = "ML_estimates";
const TWOD_IDXS_FNAME: &str = "ML_estimates_2d_idxs";
const CONTIGUOUS_ESTIMATES_FNAME: &str = "contiguous_kalman_estimates";

#[derive(Debug, Serialize)]
pub struct FilteredObservations {
    pub obj_id: u32,
    pub frame: SyncFno,
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub obs_2d_idx: u64, // index into ML_estimates_2d_idxs sequence
    pub hz_line0: f32,
    pub hz_line1: f32,
    pub hz_line2: f32,
    pub hz_line3: f32,
    pub hz_line4: f32,
    pub hz_line5: f32,
}

fn no_data_row(obj_id: u32, frame: SyncFno) -> KalmanEstimatesRow {
    let nan: f64 = std::f64::NAN;

    KalmanEstimatesRow {
        obj_id,
        frame,
        timestamp: None,
        x: nan,
        y: nan,
        z: nan,
        xvel: nan,
        yvel: nan,
        zvel: nan,
        P00: nan,
        P01: nan,
        P02: nan,
        P11: nan,
        P12: nan,
        P22: nan,
        P33: nan,
        P44: nan,
        P55: nan,
    }
}

fn compute_contiguous_kests(dirname: &std::path::Path) -> Result<()> {
    let dirpath = std::path::PathBuf::from(dirname);
    info!(
        "computing contiguous kalman estimates for {}",
        dirpath.display()
    );

    let kalman_estimates_reader = {
        let csv_path = dirpath.join(flydra_types::KALMAN_ESTIMATES_CSV_FNAME);
        let rdr = pick_csvgz_or_csv(&csv_path)?;
        csv::Reader::from_reader(rdr)
    };

    {
        // create dir if needed
        let mut csv_save_pathbuf = dirpath.clone();
        csv_save_pathbuf.push(COMPUTED_DIRNAME);
        std::fs::create_dir_all(&csv_save_pathbuf)?;
    }

    let mut contiguous_estimates_wtr = {
        let mut csv_path = dirpath;
        csv_path.push(COMPUTED_DIRNAME);
        csv_path.push(CONTIGUOUS_ESTIMATES_FNAME);
        csv_path.set_extension("csv");
        let fd = std::fs::File::create(&csv_path)?;
        csv::Writer::from_writer(fd)
    };

    let mut kest_per_obj_id = HashMap::new();

    for row in kalman_estimates_reader
        .into_deserialize()
        .early_eof_ok()
        .into_iter()
    {
        let row: KalmanEstimatesRow = row?;

        // Check if frame number is plausible. This large number is 2**63 and
        // if the frame number is this high, it suggests there was a problem
        // computing it.
        assert!(
            row.frame.0 < 9223372036854775808,
            "implausibly large frame number"
        );

        let rows_entry = &mut kest_per_obj_id.entry(row.obj_id).or_insert_with(Vec::new);
        rows_entry.push(row);
    }

    let min_frames = 2;
    for (obj_id, rows) in kest_per_obj_id.into_iter() {
        if rows.len() < min_frames {
            trace!("not enough frames for obj_id {}, skipping", obj_id);
            continue;
        }

        let start_frame = rows[0].frame;
        let stop_frame = rows[rows.len() - 1].frame;
        trace!(
            "filling in obj_id {}: {} points in frames {}-{}",
            obj_id,
            rows.len(),
            start_frame,
            stop_frame
        );

        let mut by_frame = BTreeMap::new();
        for row in rows.into_iter() {
            by_frame.insert(row.frame, row);
        }

        for frame in start_frame.0..(stop_frame.0 + 1) {
            let fno: SyncFno = frame.into();
            let row = match by_frame.remove(&fno) {
                Some(row) => row,
                None => no_data_row(obj_id, fno),
            };
            contiguous_estimates_wtr.serialize(row)?;
        }
    }

    Ok(())
}

/// Save data associations. Requires `frame` in `kalman_estimates_reader` to be ascending.
fn save_data_association_ascending<R1: Read, R2: Read, WT: Write>(
    kalman_estimates_reader: csv::Reader<R1>,
    data_assoc_reader: csv::Reader<R2>,
    mut ml_estimates_wtr: csv::Writer<WT>,
    dirpath: std::path::PathBuf,
) -> Result<()> {
    let mut twod_idxs_wtr_idx = 0;
    let mut twod_idxs_wtr = {
        let mut csv_path = dirpath;
        csv_path.push(COMPUTED_DIRNAME);
        csv_path.push(TWOD_IDXS_FNAME);
        csv_path.set_extension("vlarray_csv");
        std::fs::File::create(&csv_path)?
    };

    let mut da_iter = data_assoc_reader.into_deserialize::<DataAssocRow>();
    let mut da_row_frame_iter = AscendingGroupIter::new(&mut da_iter).early_eof_ok();

    let mut kalman_estimates_iter =
        kalman_estimates_reader.into_deserialize::<KalmanEstimatesRow>();
    let kest_frame_iter = AscendingGroupIter::new(&mut kalman_estimates_iter).early_eof_ok();
    let nan: f32 = std::f32::NAN;

    let opt_next_da_row = da_row_frame_iter.next();
    if opt_next_da_row.is_none() {
        // There is no data association data, so nothing to convert and save.
        return Ok(());
    }
    let next_da_row = opt_next_da_row.unwrap(); // get first row;
    let next_da_row = next_da_row?; // remove Result<>
    let mut next_da_row_container = Some(next_da_row);
    for kest_rows in kest_frame_iter {
        // Get the corresponding data association data for this frame.

        let next_da_row = next_da_row_container.take();
        let kest_rows = kest_rows?;

        // Gather all data association data for this frame of kalman estimates.
        let data_assoc_rows = match next_da_row {
            None => {
                // done? finish?
                let opt_next_da_row = da_row_frame_iter.next();
                if let Some(next_da_row) = opt_next_da_row {
                    let next_da_row = next_da_row?;
                    if kest_rows.group_key < next_da_row.group_key {
                        next_da_row_container = Some(next_da_row);
                        groupby::GroupedRows {
                            group_key: kest_rows.group_key,
                            rows: vec![],
                        }
                    } else {
                        assert!(kest_rows.group_key == next_da_row.group_key);
                        next_da_row
                    }
                } else {
                    error!(
                        "data association data finishes prior to kalman \
                        estimates data."
                    );
                    break;
                }
            }
            Some(next) => {
                if kest_rows.group_key == next.group_key {
                    next
                } else {
                    assert!(kest_rows.group_key < next.group_key);
                    next_da_row_container = Some(next);
                    groupby::GroupedRows {
                        group_key: kest_rows.group_key,
                        rows: vec![],
                    }
                }
            }
        };

        // Now we have corresponding data association data for this frame.
        assert!(data_assoc_rows.group_key == kest_rows.group_key);

        let mut da_by_obj_id = BTreeMap::new();
        for da_row in data_assoc_rows.rows {
            let rows_entry = &mut da_by_obj_id.entry(da_row.obj_id).or_insert_with(Vec::new);
            rows_entry.push(da_row);
        }

        for kest_row in kest_rows.rows {
            let obj_id = kest_row.obj_id;
            let da_rows = da_by_obj_id.remove(&obj_id);

            // camns_and_idxs
            let da_rows = da_rows.unwrap_or_else(|| Vec::with_capacity(0));

            let camns_and_idxs = da_rows
                .into_iter()
                .flat_map(|x| vec![x.cam_num.0, x.pt_idx].into_iter());

            let csvs = camns_and_idxs
                .into_iter()
                .map(|x| format!("{}", x))
                .join(",");
            writeln!(twod_idxs_wtr, "{}", csvs)?;

            // TODO: Here calculate x,y,z and hz_line from data2d using
            // data association data.

            // Also: calculate reprojection error and reconstruction latency.

            let row: FilteredObservations = FilteredObservations {
                obj_id,
                frame: kest_row.frame,
                x: nan,
                y: nan,
                z: nan,
                obs_2d_idx: twod_idxs_wtr_idx, // index into ML_estimates_2d_idxs sequence
                hz_line0: nan,
                hz_line1: nan,
                hz_line2: nan,
                hz_line3: nan,
                hz_line4: nan,
                hz_line5: nan,
            };
            ml_estimates_wtr.serialize(row)?;

            twod_idxs_wtr_idx += 1;
        }
    }

    Ok(())
}

/// Save data associations. Caches all data association data.
fn _save_data_association_cache_all<R1: Read, R2: Read, WT: Write>(
    kalman_estimates_reader: csv::Reader<R1>,
    data_assoc_reader: csv::Reader<R2>,
    mut ml_estimates_wtr: csv::Writer<WT>,
    dirpath: std::path::PathBuf,
) -> Result<()> {
    // Load all data association results into memory.
    let mut da_index = BTreeMap::new();
    let da_iter = data_assoc_reader.into_deserialize();
    let mut all_da_rows = Vec::new();
    for (row_num, row) in da_iter.enumerate() {
        let row: DataAssocRow = row?;
        {
            let key = (row.obj_id, row.frame);
            let entry = da_index.entry(key).or_insert_with(Vec::new);
            entry.push(row_num);
        }
        all_da_rows.push(row);
    }

    // Cache a couple values.
    let nan: f32 = std::f32::NAN;
    let empty_vec = vec![];

    let mut twod_idxs_wtr = {
        let mut csv_path = dirpath;
        csv_path.push(COMPUTED_DIRNAME);
        csv_path.push(TWOD_IDXS_FNAME);
        csv_path.set_extension("vlarray_csv");
        std::fs::File::create(&csv_path)?
    };

    // Iterate through all estimates.
    let kalman_estimates_iter = kalman_estimates_reader.into_deserialize();
    for (twod_idxs_wtr_idx, kest_row) in kalman_estimates_iter.enumerate() {
        let kest_row: KalmanEstimatesRow = kest_row?;

        let obj_id = kest_row.obj_id;

        let da_key = (kest_row.obj_id, kest_row.frame);
        let da_row_nums = da_index.get(&da_key);

        let da_row_nums = match da_row_nums {
            Some(rns) => rns,
            None => &empty_vec, //vec![],
        };

        // Get the corresponding data association data for this frame.
        let da_rows = da_row_nums
            .iter()
            .map(|row_idx: &usize| all_da_rows[*row_idx].clone());

        let camns_and_idxs = da_rows
            .into_iter()
            .flat_map(|x| vec![x.cam_num.0, x.pt_idx].into_iter());

        let csvs = camns_and_idxs
            .into_iter()
            .map(|x| format!("{}", x))
            .join(",");
        writeln!(twod_idxs_wtr, "{}", csvs)?;

        let row: FilteredObservations = FilteredObservations {
            obj_id,
            frame: kest_row.frame,
            x: nan,
            y: nan,
            z: nan,
            obs_2d_idx: twod_idxs_wtr_idx as u64, // index into ML_estimates_2d_idxs sequence
            hz_line0: nan,
            hz_line1: nan,
            hz_line2: nan,
            hz_line3: nan,
            hz_line4: nan,
            hz_line5: nan,
        };
        ml_estimates_wtr.serialize(row)?;
    }

    Ok(())
}

fn add_ml_estimates_tables(dirname: &std::path::Path) -> Result<()> {
    let dirpath = std::path::PathBuf::from(dirname);
    info!(
        "computing ML_estimates and ML_estimates_2d_idxs for {}",
        dirpath.display()
    );

    let data_assoc_reader = {
        let csv_path = dirpath.join(flydra_types::DATA_ASSOCIATE_CSV_FNAME);
        let rdr = pick_csvgz_or_csv(&csv_path)?;
        csv::Reader::from_reader(rdr)
    };

    let kalman_estimates_reader = {
        let csv_path = dirpath.join(flydra_types::KALMAN_ESTIMATES_CSV_FNAME);
        let rdr = pick_csvgz_or_csv(&csv_path)?;
        csv::Reader::from_reader(rdr)
    };

    {
        // create dir if needed
        let mut csv_save_pathbuf = dirpath.clone();
        csv_save_pathbuf.push(COMPUTED_DIRNAME);
        std::fs::create_dir_all(&csv_save_pathbuf)?;
    }

    let ml_estimates_wtr = {
        let mut csv_path = dirpath.clone();
        csv_path.push(COMPUTED_DIRNAME);
        csv_path.push(ML_ESTIMATES_FNAME);
        csv_path.set_extension("csv");
        let fd = std::fs::File::create(&csv_path)?;
        csv::Writer::from_writer(fd)
    };

    save_data_association_ascending(
        kalman_estimates_reader,
        data_assoc_reader,
        ml_estimates_wtr,
        dirpath,
    )?;
    // save_data_association_cache_all(kalman_estimates_reader,data_assoc_reader,ml_estimates_wtr,dirpath.clone())?;

    Ok(())
}

#[derive(Debug, StructOpt)]
#[structopt(name = "compute-flydra1-compat")]
struct Opt {
    /// Input and output directory
    #[structopt(parse(from_os_str))]
    dirname: std::path::PathBuf,
}

fn main() -> Result<()> {
    env_tracing_logger::init();

    let opt = Opt::from_args();

    // Here we operate on a plain directory (rather than a
    // `zip_or_dir::ZipDirArchive`).

    compute_contiguous_kests(&opt.dirname)?;
    add_ml_estimates_tables(&opt.dirname)
}
