#![allow(non_snake_case,non_camel_case_types)]

use libc::{c_uint, c_void, c_uchar, c_char, c_ushort, c_int, c_float};

pub type voidptr_t = *mut c_void;
pub type fc2Context = voidptr_t;
pub type fc2ImageImpl = voidptr_t;

pub type BOOL = c_int;

pub const FULL_32BIT_VALUE: isize = 0x7FFFFFFF;
pub const MAX_STRING_LENGTH: usize = 512;

#[repr(C)]
pub struct fc2Image {
    pub rows: c_uint,
    pub cols: c_uint,
    pub stride: c_uint,
    pub pData: *mut c_uchar,
    pub dataSize: c_uint,
    pub receivedDataSize: c_uint,
    pub format: fc2PixelFormat,
    pub bayerFormat: fc2BayerTileFormat,
    pub imageImpl: fc2ImageImpl,
}

#[repr(C)]
pub struct fc2PGRGuid {
    pub value: [c_uint; 4],
}

#[repr(C)]
pub enum fc2BusCallbackType {
    FC2_BUS_RESET,
    FC2_ARRIVAL,
    FC2_REMOVAL,
    FC2_CALLBACK_TYPE_FORCE_32BITS = FULL_32BIT_VALUE,
}

#[repr(C)]
pub enum fc2GrabMode {
    FC2_DROP_FRAMES,
    FC2_BUFFER_FRAMES,
    FC2_UNSPECIFIED_GRAB_MODE,
    FC2_GRAB_MODE_FORCE_32BITS = FULL_32BIT_VALUE,
}

pub enum fc2GrabTimeout {
    FC2_TIMEOUT_NONE = 0,
    FC2_TIMEOUT_INFINITE = -1,
    FC2_TIMEOUT_UNSPECIFIED = -2,
    FC2_GRAB_TIMEOUT_FORCE_32BITS = FULL_32BIT_VALUE,
}


#[derive(Debug)]
#[repr(C)]
pub enum fc2Error {
    FC2_ERROR_UNDEFINED = -1,
    /**< Undefined */
    FC2_ERROR_OK,
    /**< Function returned with no errors. */
    FC2_ERROR_FAILED,
    /**< General failure. */
    FC2_ERROR_NOT_IMPLEMENTED,
    /**< Function has not been implemented. */
    FC2_ERROR_FAILED_BUS_MASTER_CONNECTION,
    /**< Could not connect to Bus Master. */
    FC2_ERROR_NOT_CONNECTED,
    /**< Camera has not been connected. */
    FC2_ERROR_INIT_FAILED,
    /**< Initialization failed. */
    FC2_ERROR_NOT_INTITIALIZED,
    /**< Camera has not been initialized. */
    FC2_ERROR_INVALID_PARAMETER,
    /**< Invalid parameter passed to function. */
    FC2_ERROR_INVALID_SETTINGS,
    /**< Setting set to camera is invalid. */
    FC2_ERROR_INVALID_BUS_MANAGER,
    /**< Invalid Bus Manager object. */
    FC2_ERROR_MEMORY_ALLOCATION_FAILED,
    /**< Could not allocate memory. */
    FC2_ERROR_LOW_LEVEL_FAILURE,
    /**< Low level error. */
    FC2_ERROR_NOT_FOUND,
    /**< Device not found. */
    FC2_ERROR_FAILED_GUID,
    /**< GUID failure. */
    FC2_ERROR_INVALID_PACKET_SIZE,
    /**< Packet size set to camera is invalid. */
    FC2_ERROR_INVALID_MODE,
    /**< Invalid mode has been passed to function. */
    FC2_ERROR_NOT_IN_FORMAT7,
    /**< Error due to not being in Format7. */
    FC2_ERROR_NOT_SUPPORTED,
    /**< This feature is unsupported. */
    FC2_ERROR_TIMEOUT,
    /**< Timeout error. */
    FC2_ERROR_BUS_MASTER_FAILED,
    /**< Bus Master Failure. */
    FC2_ERROR_INVALID_GENERATION,
    /**< Generation Count Mismatch. */
    FC2_ERROR_LUT_FAILED,
    /**< Look Up Table failure. */
    FC2_ERROR_IIDC_FAILED,
    /**< IIDC failure. */
    FC2_ERROR_STROBE_FAILED,
    /**< Strobe failure. */
    FC2_ERROR_TRIGGER_FAILED,
    /**< Trigger failure. */
    FC2_ERROR_PROPERTY_FAILED,
    /**< Property failure. */
    FC2_ERROR_PROPERTY_NOT_PRESENT,
    /**< Property is not present. */
    FC2_ERROR_REGISTER_FAILED,
    /**< Register access failed. */
    FC2_ERROR_READ_REGISTER_FAILED,
    /**< Register read failed. */
    FC2_ERROR_WRITE_REGISTER_FAILED,
    /**< Register write failed. */
    FC2_ERROR_ISOCH_FAILED,
    /**< Isochronous failure. */
    FC2_ERROR_ISOCH_ALREADY_STARTED,
    /**< Isochronous transfer has already been started. */
    FC2_ERROR_ISOCH_NOT_STARTED,
    /**< Isochronous transfer has not been started. */
    FC2_ERROR_ISOCH_START_FAILED,
    /**< Isochronous start failed. */
    FC2_ERROR_ISOCH_RETRIEVE_BUFFER_FAILED,
    /**< Isochronous retrieve buffer failed. */
    FC2_ERROR_ISOCH_STOP_FAILED,
    /**< Isochronous stop failed. */
    FC2_ERROR_ISOCH_SYNC_FAILED,
    /**< Isochronous image synchronization failed. */
    FC2_ERROR_ISOCH_BANDWIDTH_EXCEEDED,
    /**< Isochronous bandwidth exceeded. */
    FC2_ERROR_IMAGE_CONVERSION_FAILED,
    /**< Image conversion failed. */
    FC2_ERROR_IMAGE_LIBRARY_FAILURE,
    /**< Image library failure. */
    FC2_ERROR_BUFFER_TOO_SMALL,
    /**< Buffer is too small. */
    FC2_ERROR_IMAGE_CONSISTENCY_ERROR,
    /**< There is an image consistency error. */
    FC2_ERROR_INCOMPATIBLE_DRIVER,
    /**< The installed driver is not compatible with the library. */
    FC2_ERROR_FORCE_32BITS = 0x7FFFFFFF, // FULL_32BIT_VALUE
}

#[repr(C)]
pub enum fc2BandwidthAllocation {
    FC2_BANDWIDTH_ALLOCATION_OFF = 0,
    FC2_BANDWIDTH_ALLOCATION_ON = 1,
    FC2_BANDWIDTH_ALLOCATION_UNSUPPORTED = 2,
    FC2_BANDWIDTH_ALLOCATION_UNSPECIFIED = 3,
    FC2_BANDWIDTH_ALLOCATION_FORCE_32BITS = FULL_32BIT_VALUE,
}

#[derive(Copy, Clone, Debug, RustcDecodable, RustcEncodable)]
#[repr(C)]
pub enum fc2InterfaceType {
    FC2_INTERFACE_IEEE1394,
    FC2_INTERFACE_USB_2,
    FC2_INTERFACE_USB_3,
    FC2_INTERFACE_GIGE,
    FC2_INTERFACE_UNKNOWN,
    FC2_INTERFACE_TYPE_FORCE_32BITS = FULL_32BIT_VALUE,
}

/** Types of low level drivers that flycapture uses. */
#[derive(Copy, Clone, Debug, RustcDecodable, RustcEncodable)]
#[repr(C)]
pub enum fc2DriverType {
    FC2_DRIVER_1394_CAM,
    /**< PGRCam.sys. */
    FC2_DRIVER_1394_PRO,
    /**< PGR1394.sys. */
    FC2_DRIVER_1394_JUJU,
    /**< firewire_core. */
    FC2_DRIVER_1394_VIDEO1394,
    /**< video1394. */
    FC2_DRIVER_1394_RAW1394,
    /**< raw1394. */
    FC2_DRIVER_USB_NONE,
    /**< No usb driver used just BSD stack. (Linux only) */
    FC2_DRIVER_USB_CAM,
    /**< PGRUsbCam.sys. */
    FC2_DRIVER_USB3_PRO,
    /**< PGRXHCI.sys. */
    FC2_DRIVER_GIGE_NONE,
    /**< no gige drivers used,MS/BSD stack. */
    FC2_DRIVER_GIGE_FILTER,
    /**< PGRGigE.sys. */
    FC2_DRIVER_GIGE_PRO,
    /**< PGRGigEPro.sys. */
    FC2_DRIVER_GIGE_LWF,
    /**< PgrLwf.sys. */
    FC2_DRIVER_UNKNOWN = -1,
    /**< Unknown driver type. */
    FC2_DRIVER_FORCE_32BITS = FULL_32BIT_VALUE,
}

#[derive(Clone, Copy, PartialEq, RustcDecodable, RustcEncodable, Debug)]
#[repr(C)]
pub enum fc2PropertyType {
    FC2_BRIGHTNESS,
    FC2_AUTO_EXPOSURE,
    FC2_SHARPNESS,
    FC2_WHITE_BALANCE,
    FC2_HUE,
    FC2_SATURATION,
    FC2_GAMMA,
    FC2_IRIS,
    FC2_FOCUS,
    FC2_ZOOM,
    FC2_PAN,
    FC2_TILT,
    FC2_SHUTTER,
    FC2_GAIN,
    FC2_TRIGGER_MODE,
    FC2_TRIGGER_DELAY,
    FC2_FRAME_RATE,
    FC2_TEMPERATURE,
    FC2_UNSPECIFIED_PROPERTY_TYPE,
    FC2_PROPERTY_TYPE_FORCE_32BITS = FULL_32BIT_VALUE,
}

#[derive(Clone, Copy, PartialEq, RustcDecodable, RustcEncodable, Debug)]
#[repr(C)]
pub enum fc2FrameRate {
    FC2_FRAMERATE_1_875,
    /**< 1.875 fps. */
    FC2_FRAMERATE_3_75,
    /**< 3.75 fps. */
    FC2_FRAMERATE_7_5,
    /**< 7.5 fps. */
    FC2_FRAMERATE_15,
    /**< 15 fps. */
    FC2_FRAMERATE_30,
    /**< 30 fps. */
    FC2_FRAMERATE_60,
    /**< 60 fps. */
    FC2_FRAMERATE_120,
    /**< 120 fps. */
    FC2_FRAMERATE_240,
    /**< 240 fps. */
    FC2_FRAMERATE_FORMAT7,
    /**< Custom frame rate for Format7 functionality. */
    FC2_NUM_FRAMERATES,
    /**< Number of possible camera frame rates. */
    FC2_FRAMERATE_FORCE_32BITS = FULL_32BIT_VALUE,
}

#[derive(Clone, Copy, PartialEq, RustcDecodable, RustcEncodable, Debug)]
#[repr(C)]
pub enum fc2VideoMode {
    FC2_VIDEOMODE_160x120YUV444,
    /**< 160x120 YUV444. */
    FC2_VIDEOMODE_320x240YUV422,
    /**< 320x240 YUV422. */
    FC2_VIDEOMODE_640x480YUV411,
    /**< 640x480 YUV411. */
    FC2_VIDEOMODE_640x480YUV422,
    /**< 640x480 YUV422. */
    FC2_VIDEOMODE_640x480RGB,
    /**< 640x480 24-bit RGB. */
    FC2_VIDEOMODE_640x480Y8,
    /**< 640x480 8-bit. */
    FC2_VIDEOMODE_640x480Y16,
    /**< 640x480 16-bit. */
    FC2_VIDEOMODE_800x600YUV422,
    /**< 800x600 YUV422. */
    FC2_VIDEOMODE_800x600RGB,
    /**< 800x600 RGB. */
    FC2_VIDEOMODE_800x600Y8,
    /**< 800x600 8-bit. */
    FC2_VIDEOMODE_800x600Y16,
    /**< 800x600 16-bit. */
    FC2_VIDEOMODE_1024x768YUV422,
    /**< 1024x768 YUV422. */
    FC2_VIDEOMODE_1024x768RGB,
    /**< 1024x768 RGB. */
    FC2_VIDEOMODE_1024x768Y8,
    /**< 1024x768 8-bit. */
    FC2_VIDEOMODE_1024x768Y16,
    /**< 1024x768 16-bit. */
    FC2_VIDEOMODE_1280x960YUV422,
    /**< 1280x960 YUV422. */
    FC2_VIDEOMODE_1280x960RGB,
    /**< 1280x960 RGB. */
    FC2_VIDEOMODE_1280x960Y8,
    /**< 1280x960 8-bit. */
    FC2_VIDEOMODE_1280x960Y16,
    /**< 1280x960 16-bit. */
    FC2_VIDEOMODE_1600x1200YUV422,
    /**< 1600x1200 YUV422. */
    FC2_VIDEOMODE_1600x1200RGB,
    /**< 1600x1200 RGB. */
    FC2_VIDEOMODE_1600x1200Y8,
    /**< 1600x1200 8-bit. */
    FC2_VIDEOMODE_1600x1200Y16,
    /**< 1600x1200 16-bit. */
    FC2_VIDEOMODE_FORMAT7,
    /**< Custom video mode for Format7 functionality. */
    FC2_NUM_VIDEOMODES,
    /**< Number of possible video modes. */
    FC2_VIDEOMODE_FORCE_32BITS = FULL_32BIT_VALUE,
}

#[derive(Clone, Copy, PartialEq, RustcDecodable, RustcEncodable, Debug)]
#[repr(C)]
pub enum fc2Mode {
    FC2_MODE_0 = 0,
    FC2_MODE_1,
    FC2_MODE_2,
    FC2_MODE_3,
    FC2_MODE_4,
    FC2_MODE_5,
    FC2_MODE_6,
    FC2_MODE_7,
    FC2_MODE_8,
    FC2_MODE_9,
    FC2_MODE_10,
    FC2_MODE_11,
    FC2_MODE_12,
    FC2_MODE_13,
    FC2_MODE_14,
    FC2_MODE_15,
    FC2_MODE_16,
    FC2_MODE_17,
    FC2_MODE_18,
    FC2_MODE_19,
    FC2_MODE_20,
    FC2_MODE_21,
    FC2_MODE_22,
    FC2_MODE_23,
    FC2_MODE_24,
    FC2_MODE_25,
    FC2_MODE_26,
    FC2_MODE_27,
    FC2_MODE_28,
    FC2_MODE_29,
    FC2_MODE_30,
    FC2_MODE_31,
    FC2_NUM_MODES,
    /**< Number of modes */
    FC2_MODE_FORCE_32BITS = FULL_32BIT_VALUE,
}

#[derive(Copy, Clone, RustcDecodable, RustcEncodable, Debug)]
#[repr(C)]
pub enum fc2PixelFormat {
    FC2_PIXEL_FORMAT_MONO8 = 0x80000000, // < 8 bits of mono information.
    FC2_PIXEL_FORMAT_411YUV8 = 0x40000000, // < YUV 4:1:1.
    FC2_PIXEL_FORMAT_422YUV8 = 0x20000000, // < YUV 4:2:2.
    FC2_PIXEL_FORMAT_444YUV8 = 0x10000000, // < YUV 4:4:4.
    FC2_PIXEL_FORMAT_RGB8 = 0x08000000, // < R = G = B = 8 bits.
    FC2_PIXEL_FORMAT_MONO16 = 0x04000000, // < 16 bits of mono information.
    FC2_PIXEL_FORMAT_RGB16 = 0x02000000, // < R = G = B = 16 bits.
    FC2_PIXEL_FORMAT_S_MONO16 = 0x01000000, // < 16 bits of signed mono information.
    FC2_PIXEL_FORMAT_S_RGB16 = 0x00800000, // < R = G = B = 16 bits signed.
    FC2_PIXEL_FORMAT_RAW8 = 0x00400000, // < 8 bit raw data output of sensor.
    FC2_PIXEL_FORMAT_RAW16 = 0x00200000, // < 16 bit raw data output of sensor.
    FC2_PIXEL_FORMAT_MONO12 = 0x00100000, // < 12 bits of mono information.
    FC2_PIXEL_FORMAT_RAW12 = 0x00080000, // < 12 bit raw data output of sensor.
    FC2_PIXEL_FORMAT_BGR = 0x80000008, // < 24 bit BGR.
    FC2_PIXEL_FORMAT_BGRU = 0x40000008, // < 32 bit BGRU.
    // 		FC2_PIXEL_FORMAT_RGB			= FC2_PIXEL_FORMAT_RGB8, /*< 24 bit RGB. */
    FC2_PIXEL_FORMAT_RGBU = 0x40000002, // < 32 bit RGBU.
    FC2_PIXEL_FORMAT_BGR16 = 0x02000001, // < R = G = B = 16 bits.
    FC2_PIXEL_FORMAT_BGRU16 = 0x02000002, // < 64 bit BGRU.
    FC2_PIXEL_FORMAT_422YUV8_JPEG = 0x40000001, // < JPEG compressed stream.
    FC2_NUM_PIXEL_FORMATS = 20, // < Number of pixel formats.
    FC2_UNSPECIFIED_PIXEL_FORMAT = 0, // < Unspecified pixel format.
}

#[derive(Copy, Clone, Debug, RustcDecodable, RustcEncodable)]
#[repr(C)]
pub enum fc2BusSpeed {
    FC2_BUSSPEED_S100, // < 100Mbits/sec.
    FC2_BUSSPEED_S200, // < 200Mbits/sec.
    FC2_BUSSPEED_S400, // < 400Mbits/sec.
    FC2_BUSSPEED_S480, // < 480Mbits/sec. Only for USB2 cameras.
    FC2_BUSSPEED_S800, // < 800Mbits/sec.
    FC2_BUSSPEED_S1600, // < 1600Mbits/sec.
    FC2_BUSSPEED_S3200, // < 3200Mbits/sec.
    FC2_BUSSPEED_S5000, // < 5000Mbits/sec. Only for USB3 cameras.
    FC2_BUSSPEED_10BASE_T, // < 10Base-T. Only for GigE cameras.
    FC2_BUSSPEED_100BASE_T, // < 100Base-T.  Only for GigE cameras.
    FC2_BUSSPEED_1000BASE_T, // < 1000Base-T (Gigabit Ethernet).  Only for GigE cameras.
    FC2_BUSSPEED_10000BASE_T, // < 10000Base-T.  Only for GigE cameras.
    FC2_BUSSPEED_S_FASTEST, // < The fastest speed available.
    FC2_BUSSPEED_ANY, // < Any speed that is available.
    FC2_BUSSPEED_SPEED_UNKNOWN = -1, // < Unknown bus speed.
    FC2_BUSSPEED_FORCE_32BITS = FULL_32BIT_VALUE,
}

#[derive(Copy, Clone, Debug, RustcDecodable, RustcEncodable)]
#[repr(C)]
pub enum fc2PCIeBusSpeed {
    FC2_PCIE_BUSSPEED_2_5, // 2.5 Gb/s
    FC2_PCIE_BUSSPEED_5_0, // 5.0 Gb/s
    FC2_PCIE_BUSSPEED_UNKNOWN = -1, // Speed is unknown
    FC2_PCIE_BUSSPEED_FORCE_32BITS = FULL_32BIT_VALUE,
}

#[derive(Copy, Clone, Debug, RustcDecodable, RustcEncodable)]
#[repr(C)]
pub enum fc2ColorProcessingAlgorithm {
    FC2_DEFAULT,
    FC2_NO_COLOR_PROCESSING,
    FC2_NEAREST_NEIGHBOR_FAST,
    FC2_EDGE_SENSING,
    FC2_HQ_LINEAR,
    FC2_RIGOROUS,
    FC2_IPP,
    FC2_DIRECTIONAL,
    FC2_COLOR_PROCESSING_ALGORITHM_FORCE_32BITS = FULL_32BIT_VALUE,
}

#[derive(Copy, Clone, RustcDecodable, RustcEncodable, Debug)]
#[repr(C)]
pub enum fc2BayerTileFormat {
    FC2_BT_NONE, // < No bayer tile format.
    FC2_BT_RGGB, // < Red-Green-Green-Blue.
    FC2_BT_GRBG, // < Green-Red-Blue-Green.
    FC2_BT_GBRG, // < Green-Blue-Red-Green.
    FC2_BT_BGGR, // < Blue-Green-Green-Red.
    FC2_BT_FORCE_32BITS = FULL_32BIT_VALUE,
}

#[repr(C)]
pub struct fc2CameraInfo {
    serialNumber: c_uint,
    interfaceType: fc2InterfaceType,
    driverType: fc2DriverType,
    isColorCamera: BOOL,
    modelName: [c_char; MAX_STRING_LENGTH],
    vendorName: [c_char; MAX_STRING_LENGTH],
    sensorInfo: [c_char; MAX_STRING_LENGTH],
    sensorResolution: [c_char; MAX_STRING_LENGTH],
    driverName: [c_char; MAX_STRING_LENGTH],
    firmwareVersion: [c_char; MAX_STRING_LENGTH],
    firmwareBuildTime: [c_char; MAX_STRING_LENGTH],
    maximumBusSpeed: fc2BusSpeed,
    pcieBusSpeed: fc2PCIeBusSpeed,
    bayerTileFormat: fc2BayerTileFormat,
    busNumber: c_ushort,
    nodeNumber: c_ushort,

    // IIDC specific information
    iidcVer: c_uint,
    configROM: fc2ConfigROM,

    // GigE specific information
    gigEMajorVersion: c_uint,
    gigEMinorVersion: c_uint,
    userDefinedName: [c_char; MAX_STRING_LENGTH],
    xmlURL1: [c_char; MAX_STRING_LENGTH],
    xmlURL2: [c_char; MAX_STRING_LENGTH],
    macAddress: fc2MACAddress,
    ipAddress: fc2IPAddress,
    subnetMask: fc2IPAddress,
    defaultGateway: fc2IPAddress,

    /** Status/Content of CCP register */
    ccpStatus: c_uint,
    /** Local Application IP Address. */
    applicationIPAddress: c_uint,
    /** Local Application port. */
    applicationPort: c_uint,

    reserved: [c_char; 16],
}

#[repr(C)]
pub struct fc2ConfigROM {
    nodeVendorId: c_uint,
    chipIdHi: c_uint,
    chipIdLo: c_uint,
    unitSpecId: c_uint,
    unitSWVer: c_uint,
    unitSubSWVer: c_uint,
    vendorUniqueInfo_0: c_uint,
    vendorUniqueInfo_1: c_uint,
    vendorUniqueInfo_2: c_uint,
    vendorUniqueInfo_3: c_uint,
    pszKeyword: [c_char; MAX_STRING_LENGTH],
    reserved: [c_int; 16],
}

#[derive(Copy, Clone, Debug, RustcDecodable, RustcEncodable)]
#[repr(C)]
pub struct fc2MACAddress {
    octets: [c_uchar; 6],
}

#[derive(Copy, Clone, Debug, RustcDecodable, RustcEncodable)]
#[repr(C)]
pub struct fc2IPAddress {
    octets: [c_uchar; 4],
}

#[derive(Copy, Clone, Default, RustcDecodable, RustcEncodable)]
#[repr(C)]
pub struct fc2Format7ImageSettings {
    pub mode: fc2Mode,
    pub offsetX: c_uint,
    pub offsetY: c_uint,
    pub width: c_uint,
    pub height: c_uint,
    pub pixelFormat: fc2PixelFormat,
    pub reserved: [c_uint; 8],
}

#[derive(Default, Copy, Clone, RustcDecodable, RustcEncodable)]
#[repr(C)]
pub struct fc2Format7Info {
    pub mode: fc2Mode,
    pub maxWidth: c_uint,
    pub maxHeight: c_uint,
    pub offsetHStepSize: c_uint,
    pub offsetVStepSize: c_uint,
    pub imageHStepSize: c_uint,
    pub imageVStepSize: c_uint,
    pub pixelFormatBitField: c_uint,
    pub vendorPixelFormatBitField: c_uint,
    pub packetSize: c_uint,
    pub minPacketSize: c_uint,
    pub maxPacketSize: c_uint,
    pub percentage: c_float,
    pub reserved: [c_uint; 16],
}

#[derive(Default, Copy, Clone)]
#[repr(C)]
pub struct fc2Format7PacketInfo {
    pub recommendedBytesPerPacket: c_uint,
    pub maxBytesPerPacket: c_uint,
    pub unitBytesPerPacket: c_uint,
    pub reserved: [c_uint; 8],
}

#[repr(C)]
pub struct fc2PropertyInfo {
// Same definition for fc2TriggerDelayInfo
    pub type_: fc2PropertyType, // in C, this field is named "type"
    pub present: BOOL,
    pub autoSupported: BOOL,
    pub manualSupported: BOOL,
    pub onOffSupported: BOOL,
    pub onePushSupported: BOOL,
    pub absValSupported: BOOL,
    pub readOutSupported: BOOL,
    pub min: c_uint,
    pub max: c_uint,
    pub absMin: c_float,
    pub absMax: c_float,
    pub pUnits: [c_char; MAX_STRING_LENGTH],
    pub pUnitAbbr: [c_char; MAX_STRING_LENGTH],
    pub reserved: [c_uint; 8],
}

#[repr(C)]
pub struct fc2Property {
// Same definition for fc2TriggerDelay
    pub type_: fc2PropertyType, // in C, this field is named "type"
    pub present: BOOL,
    pub absControl: BOOL,
    pub onePush: BOOL,
    pub onOff: BOOL,
    pub autoManualMode: BOOL,
    pub valueA: c_uint,
    pub valueB: c_uint,
    pub absValue: c_float,
    pub reserved: [c_uint; 8],
}
