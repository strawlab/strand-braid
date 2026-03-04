use tabled::{Table, Tabled, settings::style::Style};

static TIMEZONE: std::sync::OnceLock<chrono::Local> = std::sync::OnceLock::new();

#[derive(Tabled, Default)]
struct FrameByFrameRow {
    #[tabled(display = "display_data2d_distorted_rows")]
    data2d_distorted_rows: Vec<DisplayData2dDistortedRow>,
    #[tabled(display = "display_data_association")]
    data_association: Vec<DisplayDataAssocRow>,
    #[tabled(display = "display_kalman_estimates")]
    kalman_estimates: Vec<DisplayKalmanEstimates>,
}

#[derive(Tabled, Clone)]
struct DisplayKalmanEstimates {
    obj_id: u32,
    x: String,
    y: String,
    z: String,
}

impl From<&braid_types::KalmanEstimatesRow> for DisplayKalmanEstimates {
    fn from(orig: &braid_types::KalmanEstimatesRow) -> Self {
        Self {
            obj_id: orig.obj_id,
            x: format!("{:.04}", orig.x),
            y: format!("{:.04}", orig.y),
            z: format!("{:.04}", orig.z),
        }
    }
}

#[derive(Tabled, Clone)]
struct DisplayDataAssocRow {
    obj_id: u32,
    cam_num: u8,
    pt_idx: u8,
}

impl From<&braid_types::DataAssocRow> for DisplayDataAssocRow {
    fn from(orig: &braid_types::DataAssocRow) -> Self {
        Self {
            obj_id: orig.obj_id,
            cam_num: orig.cam_num.0,
            pt_idx: orig.pt_idx,
        }
    }
}

#[derive(Tabled, Clone)]
struct DisplayData2dDistortedRow {
    camn: u8,
    #[tabled(display = "display_time")]
    timestamp: Option<chrono::DateTime<chrono::Local>>,
    #[tabled(rename = "cam latency (msec)", display = "display_option")]
    cam_latency: Option<i64>,
    x: String,
    y: String,
}

impl From<braid_types::Data2dDistortedRow> for DisplayData2dDistortedRow {
    fn from(orig: braid_types::Data2dDistortedRow) -> Self {
        let timestamp: Option<chrono::DateTime<chrono::Local>> = orig.timestamp.as_ref().map(|t| {
            let dt: chrono::DateTime<chrono::Utc> = t.into();
            let tz = TIMEZONE.get().expect("Timezone not set");
            let timestamp = dt.with_timezone(tz);
            timestamp
        });

        let cam_latency = timestamp.map(|dtl| {
            let cam_received_timestamp: chrono::DateTime<chrono::Utc> =
                (&orig.cam_received_timestamp).into();
            cam_received_timestamp
                .signed_duration_since(dtl)
                .num_milliseconds()
        });
        Self {
            camn: orig.camn.0,
            timestamp,
            cam_latency,
            x: format!("{:.1}", orig.x),
            y: format!("{:.1}", orig.y),
        }
    }
}

fn display_kalman_estimates(d: &[DisplayKalmanEstimates]) -> String {
    if d.is_empty() {
        return "".to_string();
    }
    // sort by obj_id for reproducibility
    let mut d = d.to_vec();
    d.sort_by_key(|row| row.obj_id);
    Table::new(d).with(Style::empty()).to_string()
}

fn display_data_association(d: &[DisplayDataAssocRow]) -> String {
    if d.is_empty() {
        return "".to_string();
    }
    // sort by cam_num for reproducibility
    let mut d = d.to_vec();
    d.sort_by_key(|row| row.cam_num);
    Table::new(d).with(Style::empty()).to_string()
}

fn display_data2d_distorted_rows(d: &[DisplayData2dDistortedRow]) -> String {
    if d.is_empty() {
        return "".to_string();
    }
    // sort by camn for reproducibility
    let mut d = d.to_vec();
    d.sort_by_key(|row| row.camn);
    Table::new(d).with(Style::empty()).to_string()
}

fn display_time(timestamp: &Option<chrono::DateTime<chrono::Local>>) -> String {
    timestamp
        .as_ref()
        .map(|dtl| {
            let naive_time = dtl.time();
            format!("{naive_time}")
        })
        .unwrap_or_else(|| "".to_string())
}

fn display_option<T: std::fmt::Display>(opt: &Option<T>) -> String {
    opt.as_ref()
        .map(|v| format!("{}", v))
        .unwrap_or_else(|| "".to_string())
}

pub(crate) fn print_frame_by_frame(
    mut archive: braidz_parser::BraidzArchive<std::io::BufReader<std::fs::File>>,
) -> anyhow::Result<()> {
    let tz = archive
        .metadata
        .original_recording_time
        .map(|dt| dt.timezone())
        .unwrap_or_else(|| chrono::Local::now().timezone());

    TIMEZONE.set(tz).expect("Timezone already set");

    let mut top_level_rows = std::collections::BTreeMap::<u64, FrameByFrameRow>::new();

    if let Some(data_association) = &archive.data_association {
        for row in data_association.iter() {
            top_level_rows
                .entry(row.frame.0)
                .or_insert_with(Default::default)
                .data_association
                .push(row.into());
        }
    }

    for row in archive.iter_data2d_distorted()? {
        let row = row?;
        if !row.x.is_nan() {
            top_level_rows
                .entry(row.frame.try_into().unwrap())
                .or_insert_with(Default::default)
                .data2d_distorted_rows
                .push(row.into());
        }
    }

    if let Some(kalman_estimates) = &archive.kalman_estimates_table {
        for row in kalman_estimates.iter() {
            top_level_rows
                .entry(row.frame.0)
                .or_insert_with(Default::default)
                .kalman_estimates
                .push(row.into());
        }
    }

    let mut top_level_table = Table::new(top_level_rows);
    let top_level_table = top_level_table.with(Style::modern());
    println!("{top_level_table}");
    Ok(())
}
