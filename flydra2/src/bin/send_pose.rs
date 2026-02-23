use chrono::Local;
use std::time::Instant;

use braid_types::{FlydraFloatTimestampLocal, KalmanEstimatesRow, SyncFno, Triggerbox};
use flydra2::{new_model_server, Result, SendType, TimeDataPassthrough};

#[tokio::main]
async fn main() -> Result<()> {
    if std::env::var_os("RUST_LOG").is_none() {
        // TODO: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::set_var("RUST_LOG", "info") };
    }
    let _tracing_guard = env_tracing_logger::init();

    let addr: std::net::SocketAddr = braid_types::DEFAULT_MODEL_SERVER_ADDR.parse().unwrap();
    tracing::info!("starting send_pose server at {addr}");

    let (data_tx, data_rx) = tokio::sync::mpsc::channel(50);

    let model_server_future = new_model_server(data_rx, addr);

    tokio::spawn(model_server_future);

    let starti = Instant::now();

    let start = Local::now();
    let start = Some(FlydraFloatTimestampLocal::<Triggerbox>::from_dt(&start));

    let mut record = KalmanEstimatesRow {
        obj_id: 0,
        frame: 0.into(),
        timestamp: start.clone(),
        x: 0.0,
        y: 0.0,
        z: 0.0,
        xvel: 0.0,
        yvel: 0.0,
        zvel: 0.0,
        P00: 0.0,
        P01: 0.0,
        P02: 0.0,
        P11: 0.0,
        P12: 0.0,
        P22: 0.0,
        P33: 0.0,
        P44: 0.0,
        P55: 0.0,
    };
    let tdpt = TimeDataPassthrough::new(SyncFno(0), &start);
    data_tx
        .send((SendType::Birth(record.clone().into()), tdpt.clone()))
        .await
        .unwrap();

    // Create a stream to update pose
    let mut interval_stream = tokio::time::interval(std::time::Duration::from_millis(100));

    // Update the pose
    let stream_done_future = async move {
        loop {
            interval_stream.tick().await;
            let dur = starti.elapsed();
            let dur_f64 = (dur.as_secs() as f64) + (1e-9 * dur.subsec_nanos() as f64);

            let now = Some(FlydraFloatTimestampLocal::from_dt(&Local::now()));
            record.frame.0 += 1;
            record.timestamp = now;
            record.x = dur_f64.sin();
            record.y = dur_f64.cos();
            let tdpt = TimeDataPassthrough::new(record.frame, &record.timestamp);
            data_tx
                .send((SendType::Update(record.clone().into()), tdpt.clone()))
                .await
                .expect("send_update");
            data_tx
                .send((SendType::EndOfFrame(record.frame), tdpt.clone()))
                .await
                .expect("send_eof");
        }
    };

    stream_done_future.await;

    Ok(())
}
