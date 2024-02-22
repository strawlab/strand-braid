use color_eyre::{
    eyre::{self as anyhow},
    Result,
};
use std::collections::{BTreeMap, BTreeSet};

use flydra_types::{PerCamSaveData, RawCamName};

use crate::{
    config::{BraidRetrackVideoConfig, CameraCalibrationSource, TrackingParametersSource},
    PerCamRenderFrame,
};

pub(crate) struct BraidStorage {
    pub(crate) cam_manager: flydra2::ConnectedCamerasManager,
    pub(crate) frame_data_tx: tokio::sync::mpsc::Sender<flydra2::StreamItem>,
    pub(crate) output_braidz_path: std::path::PathBuf,
}

impl BraidStorage {
    pub(crate) async fn new(
        cfg: &BraidRetrackVideoConfig,
        b: &crate::config::BraidzOutputConfig,
        tracking_parameters: Option<flydra_types::TrackingParams>,
        sources: &[crate::CameraSource],
        all_expected_cameras: BTreeSet<RawCamName>,
        expected_framerate: Option<f32>,
    ) -> Result<Self> {
        let output_braidz_path = std::path::PathBuf::from(&b.filename);
        let output_dirname =
            if output_braidz_path.extension() == Some(std::ffi::OsStr::new("braidz")) {
                let mut output_dirname = output_braidz_path.clone();
                output_dirname.set_extension("braid"); // replace .braidz -> .braid
                output_dirname
            } else {
                anyhow::bail!("extension of braidz output file must be '.braidz'.");
            };

        let recon = match cfg.processing_config.camera_calibration_source {
            CameraCalibrationSource::None => None,
        };

        let tracking_params: flydra_types::TrackingParams = match cfg
            .processing_config
            .tracking_parameters_source
        {
            TrackingParametersSource::Default => match cfg.input_video.len() {
                1 => flydra_types::default_tracking_params_flat_3d(),
                _ => flydra_types::default_tracking_params_full_3d(),
            },
            TrackingParametersSource::CopyExisting => {
                if let Some(tracking_parameters) = tracking_parameters.as_ref() {
                    tracking_parameters.clone()
                } else {
                    anyhow::bail!(
                                        "No tracking parameter source needed because braidz output is 'CopyExisting'."
                                    );
                }
            }
        };

        let signal_all_cams_present =
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let signal_all_cams_synced = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));

        let braidz_per_cam_save_data: BTreeMap<RawCamName, PerCamSaveData> = sources
            .iter()
            .map(|source| {
                let raw_cam_name = source.per_cam_render.raw_name.clone();
                let current_image_png = source.per_cam_render.frame0_png_buf.clone();

                (
                    raw_cam_name,
                    PerCamSaveData {
                        current_image_png,
                        cam_settings_data: None,
                        feature_detect_settings: None,
                    },
                )
            })
            .collect();

        let mut cam_manager = flydra2::ConnectedCamerasManager::new(
            &recon,
            all_expected_cameras.clone(),
            signal_all_cams_present,
            signal_all_cams_synced,
            None,
        );

        for raw_cam_name in all_expected_cameras.iter() {
            let no_server = flydra_types::BuiServerInfo::NoServer;
            cam_manager
                .register_new_camera(&raw_cam_name, &no_server, None)
                .map_err(|msg| anyhow::anyhow!("Error registering new camera: {msg}"))?;
        }

        let (frame_data_tx, frame_data_rx) = tokio::sync::mpsc::channel(10);
        let frame_data_rx = tokio_stream::wrappers::ReceiverStream::new(frame_data_rx);
        let save_empty_data2d = true;
        let ignore_latency = true;
        let coord_processor = flydra2::CoordProcessor::new(
            flydra2::CoordProcessorConfig {
                tracking_params,
                save_empty_data2d,
                ignore_latency,
                mini_arena_debug_image_dir: None,
            },
            cam_manager.clone(),
            recon.clone(),
            flydra2::BraidMetadataBuilder::saving_program_name("braid-process-video"),
        )?;

        let save_cfg = flydra2::StartSavingCsvConfig {
            out_dir: output_dirname.to_path_buf(),
            local: None,
            git_rev: "<impossible git rev>".into(),
            fps: expected_framerate,
            per_cam_data: braidz_per_cam_save_data,
            print_stats: true,
            save_performance_histograms: false,
        };

        coord_processor
            .braidz_write_tx
            .send(flydra2::SaveToDiskMsg::StartSavingCsv(save_cfg))
            .await
            .unwrap();

        let coord_proc_fut = coord_processor.consume_stream(frame_data_rx, expected_framerate);
        tokio::spawn(coord_proc_fut);

        Ok(Self {
            cam_manager,
            frame_data_tx,
            output_braidz_path,
        })
    }
    pub(crate) async fn render_frame(
        &mut self,
        out_fno: usize,
        synced_data: &crate::SyncedPictures,
        all_cam_render_data: &[PerCamRenderFrame<'_>],
    ) -> Result<()> {
        for cam_render_data in all_cam_render_data.iter() {
            let raw_cam_name = cam_render_data.p.raw_name.clone();
            let cam_num = self.cam_manager.cam_num(&raw_cam_name).unwrap();

            let trigger_timestamp = synced_data
                .braidz_info
                .as_ref()
                .and_then(|bi| bi.trigger_timestamp.clone());

            let frame_data = flydra2::FrameData::new(
                raw_cam_name,
                cam_num,
                flydra_types::SyncFno(out_fno.try_into().unwrap()),
                trigger_timestamp,
                cam_render_data.pts_chrono.into(),
                None,
                None,
            );

            let points: Vec<_> = cam_render_data
                .points
                .iter()
                .enumerate()
                .map(|(idx, xy)| {
                    let pt = flydra_types::FlydraRawUdpPoint {
                        x0_abs: *xy.0,
                        y0_abs: *xy.1,
                        area: std::f64::NAN,
                        maybe_slope_eccentricty: None,
                        cur_val: 0,
                        mean_val: std::f64::NAN,
                        sumsqf_val: std::f64::NAN,
                    };
                    flydra2::NumberedRawUdpPoint {
                        idx: idx.try_into().unwrap(),
                        pt,
                    }
                })
                .collect();

            let fdp = flydra2::FrameDataAndPoints { frame_data, points };

            match self
                .frame_data_tx
                .send(flydra2::StreamItem::Packet(fdp))
                .await
            {
                Ok(()) => {}
                Err(e) => return Err(e.into()),
            }
        }
        Ok(())
    }
}
