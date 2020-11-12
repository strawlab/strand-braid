#[macro_use]
extern crate log;
extern crate env_logger;
extern crate flycap2;
extern crate libflycapture2_sys;

use flycap2::{get_num_cameras, get_guid_for_index, FlycapContext, Result, get_lowest_pixel_format};
use libflycapture2_sys as ffi;
use std::path::PathBuf;

fn main() -> Result<()> {
    if std::env::var_os("RUST_LOG").is_none() {
        std::env::set_var("RUST_LOG", "info");
    }

    env_logger::init();

    let n_cams = get_num_cameras()?;
    info!("{} camera(s) found", n_cams);

    for i in 0..n_cams {
        let guid = get_guid_for_index(i)?;
        info!("cam {}: {:?}", i, guid);
        let mut camera = FlycapContext::new(guid)?;

        let mut format7_info = ffi::_fc2Format7Info::default();
        format7_info.mode = ffi::_fc2Mode::FC2_MODE_0;
        let (format7_info, is_supported) = camera.get_format7_info(format7_info)?;
        info!("  format7_info={:?} is_supported={}", format7_info, is_supported);

        let mut settings = ffi::_fc2Format7ImageSettings::default();
        settings.mode = format7_info.mode;
        settings.width = format7_info.maxWidth;
        settings.height = format7_info.maxHeight;
        let pixel_formats = format7_info.pixelFormatBitField;
        settings.pixelFormat = get_lowest_pixel_format(pixel_formats);

        let (settings, is_supported, packet_info) = camera
            .validate_format7_settings(settings.into())?;

        info!("  settings={:?} is_supported={}, packet_info={:?}", settings, is_supported, packet_info);

        let packet_info: ffi::_fc2Format7PacketInfo = packet_info.into();
        let packet_size = packet_info.recommendedBytesPerPacket;
        camera.set_format7_configuration_packet(settings, packet_size)?;

        camera.start_capture()?;
        for i in 0..15 {
            let im = camera.retrieve_buffer()?;
            let filename = format!("image{:02}.png",i);
            info!("{} {:?}", filename, im.get_timestamp()?);
            let bgr_im = im.convert_to(ffi::_fc2PixelFormat::FC2_PIXEL_FORMAT_BGR)?;
            let filename = PathBuf::from(filename);
            bgr_im.save_to(&filename, ffi::_fc2ImageFileFormat::FC2_PNG )?;
        }

    }
    Ok(())
}
