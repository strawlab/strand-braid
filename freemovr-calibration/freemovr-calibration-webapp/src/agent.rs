use serde::{Deserialize, Serialize};
use wasm_bindgen::UnwrapThrowExt;

use crate::{VDispInfo, EXR_COMMENT};

pub enum MyWorkerMsg {}

#[derive(Serialize, Deserialize, Debug)]
pub enum MyWorkerRequest {
    CalcExr(freemovr_calibration::PinholeCalData),
    CalcAdvancedExr(Vec<u8>),
    CalcCsv(freemovr_calibration::PinholeCalData),
}

#[derive(Serialize, Deserialize, Debug)]
pub enum MyWorkerResponse {
    ExrData(Result<Vec<u8>, String>),
    CsvData(Result<Vec<u8>, String>),
    AdvancedExrData(Result<Vec<u8>, String>),
}

pub struct MyWorker {}

impl yew_agent::worker::Worker for MyWorker {
    type Message = MyWorkerMsg;
    type Input = MyWorkerRequest;
    type Output = MyWorkerResponse;

    fn create(_scope: &yew_agent::worker::WorkerScope<Self>) -> Self {
        Self {}
    }

    fn update(&mut self, _scope: &yew_agent::worker::WorkerScope<Self>, msg: Self::Message) {
        match msg {}
    }

    fn received(
        &mut self,
        scope: &yew_agent::worker::WorkerScope<Self>,
        msg: Self::Input,
        id: yew_agent::worker::HandlerId,
    ) {
        let (save_debug_images, show_mask) = (false, false);

        match msg {
            MyWorkerRequest::CalcExr(src_data) => {
                let vdisp_data = match freemovr_calibration::compute_vdisp_images(
                    &src_data,
                    save_debug_images,
                    show_mask,
                ) {
                    Ok(mut vdisp_data) => vdisp_data.remove(0),
                    Err(e) => {
                        scope.respond(id, MyWorkerResponse::ExrData(Err(format!("{}", e))));
                        return;
                    }
                };

                let visp_info_vec: Vec<&VDispInfo> = vec![&vdisp_data];
                let float_image = match freemovr_calibration::merge_vdisp_images(
                    &visp_info_vec,
                    &src_data,
                    save_debug_images,
                    show_mask,
                ) {
                    Ok(float_image) => float_image,
                    Err(e) => {
                        scope.respond(id, MyWorkerResponse::ExrData(Err(format!("{}", e))));
                        return;
                    }
                };

                let mut exr_writer = freemovr_calibration::ExrWriter::new();
                exr_writer.update(&float_image, EXR_COMMENT);
                let exr_buf = exr_writer.buffer();

                scope.respond(id, MyWorkerResponse::ExrData(Ok(exr_buf)));
            }
            MyWorkerRequest::CalcCsv(src_data) => {
                use freemovr_calibration::PinholeCal;
                let trimesh = src_data.geom_as_trimesh().unwrap_throw();

                let pinhole_fits = src_data.pinhole_fits();
                assert!(pinhole_fits.len() == 1);
                let (_name, cam) = &pinhole_fits[0];

                let mut csv_buf = Vec::<u8>::new();

                let jsdate = js_sys::Date::new_0();
                let iso8601_dt_str: String = jsdate.to_iso_string().into();

                let tz_offset_minutes = jsdate.get_timezone_offset();

                // get correct UTC datetime
                let created_at: Option<chrono::DateTime<chrono::Utc>> =
                    chrono::DateTime::parse_from_rfc3339(&iso8601_dt_str)
                        .ok()
                        .map(|dt| dt.with_timezone(&chrono::Utc));

                let offset =
                    chrono::FixedOffset::west_opt((tz_offset_minutes * 60.0) as i32).unwrap();
                let created_at = created_at.map(|dt| dt.with_timezone(&offset));

                // TODO: why does chrono save this without the timezone offset information?
                match freemovr_calibration::export_to_csv(&mut csv_buf, cam, trimesh, created_at) {
                    Ok(()) => {}
                    Err(e) => {
                        scope.respond(id, MyWorkerResponse::CsvData(Err(format!("{}", e))));
                        return;
                    }
                }
                scope.respond(id, MyWorkerResponse::CsvData(Ok(csv_buf)));
            }
            MyWorkerRequest::CalcAdvancedExr(raw_buf) => {
                let save_debug_images = false;
                let mut exr_buf = Vec::<u8>::new();
                let reader = std::io::Cursor::new(raw_buf.as_slice());
                match freemovr_calibration::csv2exr(
                    reader,
                    &mut exr_buf,
                    save_debug_images,
                    EXR_COMMENT,
                ) {
                    Ok(()) => {}
                    Err(e) => {
                        scope.respond(id, MyWorkerResponse::AdvancedExrData(Err(format!("{}", e))));
                        return;
                    }
                }
                scope.respond(id, MyWorkerResponse::AdvancedExrData(Ok(exr_buf)));
            }
        }
    }
}
