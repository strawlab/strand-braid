use reactive_cam::Frame;
use super::observation::{Observation, ObservedFeature, Timestamp};
use super::config::{self, DarkOrLight};

use ipp::{Ipp, IppImage, DataType, Chan, IppiSize};
use machine_vision_formats::PixelFormat;

lazy_static! {
    static ref IPP: Ipp = Ipp::new().unwrap();
}

struct CameraFrameIppImage<'a> {
    frame: &'a Frame,
}

impl<'a> CameraFrameIppImage<'a> {
    fn new(frame: &'a Frame) -> Result<CameraFrameIppImage<'a>, String> {
        match frame.pixel_format {
            PixelFormat::MONO8 |
            PixelFormat::BayerRG8 |
            PixelFormat::BayerBG8 |
            PixelFormat::BayerGB8 |
            PixelFormat::BayerGR8 => Ok(CameraFrameIppImage { frame: frame }),
            _ => Err("unsupported pixel_format".to_string()),
        }
    }
}

impl<'a> IppImage for CameraFrameIppImage<'a> {
    type D = u8;

    #[inline]
    fn dtype(&self) -> DataType {
        DataType::D8u
    }

    #[inline]
    fn chan(&self) -> Chan {
        Chan::C1R
    }

    #[inline]
    fn data(&self) -> &[u8] {
        &self.frame.image_data
    }

    #[inline]
    fn stride(&self) -> i32 {
        self.frame.stride as i32
    }

    #[inline]
    fn width(&self) -> usize {
        self.frame.roi.width as usize
    }

    #[inline]
    fn height(&self) -> usize {
        self.frame.roi.height as usize
    }
}

pub fn process_frame(frame: &Frame, cfg: &config::ImageProcessingConfig) -> Observation {
    let im = CameraFrameIppImage::new(frame).expect("converted to Ipp Image");
    let size = IppiSize::new(im.width(), im.height());
    let (min, max) = IPP.min_max(&im, &size).unwrap();
    let feature = match cfg.detect_dark_or_light {
        DarkOrLight::Dark => ObservedFeature::new(min.x as f32, min.y as f32, None),
        DarkOrLight::Light => ObservedFeature::new(max.x as f32, max.y as f32, None),
    };
    let timestamp = Timestamp::new_from_now();
    Observation::new(timestamp, Some(feature))
}
