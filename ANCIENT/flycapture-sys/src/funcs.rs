use libc::{c_uint, c_uchar, c_float};

use super::defs::*;

extern "C" {
    pub fn fc2CreateContext(pContext: *mut fc2Context) -> fc2Error;
    pub fn fc2CreateGigEContext(pContext: *mut fc2Context) -> fc2Error;
    pub fn fc2DestroyContext(context: fc2Context) -> fc2Error;
    pub fn fc2FireBusReset(context: fc2Context, pGuid: *const fc2PGRGuid) -> fc2Error;

    pub fn fc2GetNumOfCameras(context: fc2Context, pNumCameras: *mut c_uint) -> fc2Error;

    pub fn fc2GetCameraFromIndex(context: fc2Context,
                                 index: c_uint,
                                 pGuid: *mut fc2PGRGuid)
                                 -> fc2Error;

    pub fn fc2Connect(context: fc2Context, pGuid: *const fc2PGRGuid) -> fc2Error;
    pub fn fc2Disconnect(context: fc2Context) -> fc2Error;

    pub fn fc2GetVideoModeAndFrameRateInfo(context: fc2Context,
                                           videoMode: fc2VideoMode,
                                           frameRate: fc2FrameRate,
                                           pSupported: *mut BOOL)
                                           -> fc2Error;
    pub fn fc2SetVideoModeAndFrameRate(context: fc2Context,
                                       videoMode: fc2VideoMode,
                                       frameRate: fc2FrameRate)
                                       -> fc2Error;

    pub fn fc2GetFormat7Info(context: fc2Context,
                             info: *mut fc2Format7Info,
                             pSupported: *mut BOOL)
                             -> fc2Error;
    pub fn fc2SetFormat7ConfigurationPacket(context: fc2Context,
                                            imageSettings: *mut fc2Format7ImageSettings,
                                            packetSize: c_uint)
                                            -> fc2Error;
    pub fn fc2ValidateFormat7Settings(context: fc2Context,
                                      imageSettings: *mut fc2Format7ImageSettings,
                                      settingsAreValid: *mut BOOL,
                                      packetInfo: *mut fc2Format7PacketInfo)
                                      -> fc2Error;
    pub fn fc2GetFormat7Configuration(context: fc2Context,
                                      imageSettings: *mut fc2Format7ImageSettings,
                                      packetSize: *mut c_uint,
                                      percentage: *mut c_float)
                                      -> fc2Error;

    pub fn fc2StartCapture(context: fc2Context) -> fc2Error;
    pub fn fc2StopCapture(context: fc2Context) -> fc2Error;

    pub fn fc2CreateImage(pImage: *mut fc2Image) -> fc2Error;
    pub fn fc2DestroyImage(image: *mut fc2Image) -> fc2Error;
    pub fn fc2SetImageData(pImage: *mut fc2Image,
                           pData: *mut c_uchar,
                           dataSize: c_uint)
                           -> fc2Error;

    pub fn fc2RetrieveBuffer(context: fc2Context, pImage: *mut fc2Image) -> fc2Error;

    pub fn fc2GetCameraInfo(context: fc2Context, pCameraInfo: *mut fc2CameraInfo) -> fc2Error;

    pub fn fc2GetPropertyInfo(context: fc2Context, propInfo: *mut fc2PropertyInfo) -> fc2Error;
    pub fn fc2GetProperty(context: fc2Context, prop: *mut fc2Property) -> fc2Error;
    pub fn fc2SetProperty(context: fc2Context, prop: *mut fc2Property) -> fc2Error;
}
