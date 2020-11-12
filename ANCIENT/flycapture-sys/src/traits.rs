/// Trait implementations.

// Arguably these implementations do not belong in a -sys crate, but they cannot be put into
// another crate because of Rust's restrictions against implementing traits for types when neither
// the trait nor type is defined in that crate.

use super::defs::{fc2VideoMode, fc2PixelFormat, fc2BayerTileFormat,
                  fc2Mode, fc2Format7Info, fc2Image, fc2PropertyInfo,
                  fc2PropertyType, MAX_STRING_LENGTH};
use super::std;

pub trait HasResolution {
    fn get_resolution(&self) -> Option<(u16, u16)>;
}

impl HasResolution for fc2VideoMode {
    fn get_resolution(&self) -> Option<(u16, u16)> {
        match *self {
            fc2VideoMode::FC2_VIDEOMODE_160x120YUV444 => Some((160, 120)),
            fc2VideoMode::FC2_VIDEOMODE_320x240YUV422 => Some((320, 240)),
            fc2VideoMode::FC2_VIDEOMODE_640x480YUV411 => Some((640, 480)),
            fc2VideoMode::FC2_VIDEOMODE_640x480YUV422 => Some((640, 480)),
            fc2VideoMode::FC2_VIDEOMODE_640x480RGB => Some((640, 480)),
            fc2VideoMode::FC2_VIDEOMODE_640x480Y8 => Some((640, 480)),
            fc2VideoMode::FC2_VIDEOMODE_640x480Y16 => Some((640, 480)),
            fc2VideoMode::FC2_VIDEOMODE_800x600YUV422 => Some((800, 600)),
            fc2VideoMode::FC2_VIDEOMODE_800x600RGB => Some((800, 600)),
            fc2VideoMode::FC2_VIDEOMODE_800x600Y8 => Some((800, 600)),
            fc2VideoMode::FC2_VIDEOMODE_800x600Y16 => Some((800, 600)),
            fc2VideoMode::FC2_VIDEOMODE_1024x768YUV422 => Some((1024, 768)),
            fc2VideoMode::FC2_VIDEOMODE_1024x768RGB => Some((1024, 768)),
            fc2VideoMode::FC2_VIDEOMODE_1024x768Y8 => Some((1024, 768)),
            fc2VideoMode::FC2_VIDEOMODE_1024x768Y16 => Some((1024, 768)),
            fc2VideoMode::FC2_VIDEOMODE_1280x960YUV422 => Some((1280, 960)),
            fc2VideoMode::FC2_VIDEOMODE_1280x960RGB => Some((1280, 960)),
            fc2VideoMode::FC2_VIDEOMODE_1280x960Y8 => Some((1280, 960)),
            fc2VideoMode::FC2_VIDEOMODE_1280x960Y16 => Some((1280, 960)),
            fc2VideoMode::FC2_VIDEOMODE_1600x1200YUV422 => Some((1600, 1200)),
            fc2VideoMode::FC2_VIDEOMODE_1600x1200RGB => Some((1600, 1200)),
            fc2VideoMode::FC2_VIDEOMODE_1600x1200Y8 => Some((1600, 1200)),
            fc2VideoMode::FC2_VIDEOMODE_1600x1200Y16 => Some((1600, 1200)),
            fc2VideoMode::FC2_VIDEOMODE_FORMAT7 => None,
            fc2VideoMode::FC2_NUM_VIDEOMODES => None,
            fc2VideoMode::FC2_VIDEOMODE_FORCE_32BITS => None,
        }
    }
}

impl std::fmt::Debug for fc2Format7Info {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f,
               "fc2Format7Info {{ mode: {:?}
                 maxWidth: {:?}
                 \
                maxHeight: {:?}
                 offsetHStepSize: {:?}
                 \
                offsetVStepSize: {:?}
                 imageHStepSize: {:?}
                 \
                imageVStepSize: {:?}
                 pixelFormatBitField: {:?}
                 \
                vendorPixelFormatBitField: {:?}
                 packetSize: {:?}
                 \
                minPacketSize: {:?}
                 maxPacketSize: {:?}
                 \
                percentage: {:?}
               }}",
               self.mode,
               self.maxWidth,
               self.maxHeight,
               self.offsetHStepSize,
               self.offsetVStepSize,
               self.imageHStepSize,
               self.imageVStepSize,
               self.pixelFormatBitField,
               self.vendorPixelFormatBitField,
               self.packetSize,
               self.minPacketSize,
               self.maxPacketSize,
               self.percentage)
    }
}

impl std::default::Default for fc2PixelFormat {
    fn default() -> fc2PixelFormat {
        fc2PixelFormat::FC2_UNSPECIFIED_PIXEL_FORMAT
    }
}

impl std::default::Default for fc2BayerTileFormat {
    fn default() -> fc2BayerTileFormat {
        fc2BayerTileFormat::FC2_BT_NONE
    }
}

impl std::default::Default for fc2Image {
    fn default() -> fc2Image {
        fc2Image {
            rows: 0,
            cols: 0,
            stride: 0,
            pData: std::ptr::null_mut(),
            dataSize: 0,
            receivedDataSize: 0,
            format: fc2PixelFormat::default(),
            bayerFormat: fc2BayerTileFormat::default(),
            imageImpl: std::ptr::null_mut(),
        }
    }
}

impl std::default::Default for fc2Mode {
    fn default() -> fc2Mode {
        fc2Mode::FC2_MODE_0
    }
}

impl std::default::Default for fc2PropertyInfo {
    fn default() -> fc2PropertyInfo {
        fc2PropertyInfo {
            type_: fc2PropertyType::FC2_UNSPECIFIED_PROPERTY_TYPE,
            present: 0,
            autoSupported: 0,
            manualSupported: 0,
            onOffSupported: 0,
            onePushSupported: 0,
            absValSupported: 0,
            readOutSupported: 0,
            min: 0,
            max: 0,
            absMin: 0.0,
            absMax: 0.0,
            pUnits: [0; MAX_STRING_LENGTH],
            pUnitAbbr: [0; MAX_STRING_LENGTH],
            reserved: [0, 0, 0, 0, 0, 0, 0, 0],
        }
    }
}
