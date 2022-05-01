use chrono::Local;
use log::info;
use std::{sync::Arc, time::Instant};

use flydra2::{new_model_server, Result, SendType, TimeDataPassthrough};
use flydra_types::{FlydraFloatTimestampLocal, KalmanEstimatesRow, SyncFno, Triggerbox};

fn main() -> Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    let runtime = Arc::new(runtime);
    runtime.block_on(inner(runtime.handle().clone()))
}

async fn inner(rt_handle: tokio::runtime::Handle) -> Result<()> {
    env_logger::init();

    let addr = flydra_types::DEFAULT_MODEL_SERVER_ADDR.parse().unwrap();
    info!("send_pose server at {}", addr);
    let info = flydra_types::StaticMainbrainInfo {
        name: env!("CARGO_PKG_NAME").into(),
        version: env!("CARGO_PKG_VERSION").into(),
    };

    let (_quit_trigger, valve) = stream_cancel::Valve::new();

    let (data_tx, data_rx) = tokio::sync::mpsc::channel(50);

    new_model_server(data_rx, valve, None, &addr, info, rt_handle).await?;

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
