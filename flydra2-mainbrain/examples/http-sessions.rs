extern crate env_logger;
extern crate parking_lot;
extern crate tokio;

extern crate ci2_remote_control;
extern crate flydra2_mainbrain;

use flydra2_mainbrain::HttpSessionHandler;

fn main() -> Result<(), ()> {
    env_logger::init();

    let mut runtime = tokio::runtime::Runtime::new().expect("runtime");

    let mut cam_manager = flydra2::ConnectedCamerasManager::new(&None);

    let cam_id = "cam1";
    let orig_cam_name = flydra_types::RawCamName::new(cam_id.to_string());
    let ros_cam_name = flydra_types::RosCamName::new(cam_id.to_string());
    let server = flydra_types::CamHttpServerInfo::NoServer;

    cam_manager.register_new_camera(&orig_cam_name, &server, &ros_cam_name);

    let mut http_session_handler = HttpSessionHandler::new(cam_manager);

    let cm = rust_cam_bui_types::ClockModel {
        gain: 1.0,
        offset: 0.0,
        residuals: 0.0,
        n_measurements: 0,
    };
    let fut = http_session_handler.send_clock_model(&ros_cam_name, Some(cm));
    runtime.block_on(fut).expect("runtime.block_on()");
    Ok(())
}
