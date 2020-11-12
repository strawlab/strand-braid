#[derive(Debug)]
pub struct PylonError(String);

impl From<cxx::Exception> for PylonError {
    fn from(orig: cxx::Exception) -> PylonError {
        PylonError(orig.what().into())
    }
}

impl From<std::str::Utf8Error> for PylonError {
    fn from(_: std::str::Utf8Error) -> PylonError {
        PylonError("Cannot convert C++ string to UTF-8".to_string())
    }
}

impl std::fmt::Display for PylonError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "PylonError({})", self.0)
    }
}

impl std::error::Error for PylonError {}

type PylonResult<T> = Result<T, PylonError>;

#[cxx::bridge(namespace = Pylon)]
mod ffi {

    #[repr(u32)]
    enum TimeoutHandling {
        Return,
        ThrowException,
    }

    extern "C" {
        include!("pylon/PylonIncludes.h");
        include!("pylon/gige/BaslerGigECamera.h");
        include!("catcher.h");
        include!("pylon-cxx-rs.h");

        type CInstantCamera;
        type CDeviceInfo;
        type CGrabResultPtr;
        type TimeoutHandling;
        type CBooleanParameter;
        type CIntegerParameter;
        type CFloatParameter;
        type CEnumParameter;

        fn PylonInitialize();
        fn PylonTerminate(ShutDownLogging: bool);

        fn tl_factory_create_first_device() -> Result<UniquePtr<CInstantCamera>>;
        fn tl_factory_create_device(device_info: &CDeviceInfo)
            -> Result<UniquePtr<CInstantCamera>>;
        fn tl_factory_enumerate_devices() -> Result<UniquePtr<CxxVector<CDeviceInfo>>>;

        fn instant_camera_get_device_info(
            camera: &UniquePtr<CInstantCamera>,
        ) -> UniquePtr<CDeviceInfo>;
        fn instant_camera_open(camera: &UniquePtr<CInstantCamera>) -> Result<()>;
        fn instant_camera_is_open(camera: &UniquePtr<CInstantCamera>) -> Result<bool>;
        fn instant_camera_close(camera: &UniquePtr<CInstantCamera>) -> Result<()>;
        fn instant_camera_start_grabbing(camera: &UniquePtr<CInstantCamera>) -> Result<()>;
        fn instant_camera_stop_grabbing(camera: &UniquePtr<CInstantCamera>) -> Result<()>;
        fn instant_camera_start_grabbing_with_count(
            camera: &UniquePtr<CInstantCamera>,
            count: u32,
        ) -> Result<()>;
        fn instant_camera_is_grabbing(camera: &UniquePtr<CInstantCamera>) -> bool;
        fn instant_camera_retrieve_result(
            camera: &UniquePtr<CInstantCamera>,
            timeout_ms: u32,
            grab_result: &mut UniquePtr<CGrabResultPtr>,
            timeout_handling: TimeoutHandling,
        ) -> Result<bool>;

        fn node_map_get_boolean_parameter(
            camera: &UniquePtr<CInstantCamera>,
            name: &str,
        ) -> Result<UniquePtr<CBooleanParameter>>;
        fn node_map_get_integer_parameter(
            camera: &UniquePtr<CInstantCamera>,
            name: &str,
        ) -> Result<UniquePtr<CIntegerParameter>>;
        fn node_map_get_float_parameter(
            camera: &UniquePtr<CInstantCamera>,
            name: &str,
        ) -> Result<UniquePtr<CFloatParameter>>;
        fn node_map_get_enum_parameter(
            camera: &UniquePtr<CInstantCamera>,
            name: &str,
        ) -> Result<UniquePtr<CEnumParameter>>;

        fn boolean_node_get_value(boolean_node: &UniquePtr<CBooleanParameter>) -> Result<bool>;
        fn boolean_node_set_value(
            boolean_node: &UniquePtr<CBooleanParameter>,
            value: bool,
        ) -> Result<()>;

        fn integer_node_get_unit(
            node: &UniquePtr<CIntegerParameter>,
        ) -> Result<UniquePtr<CxxString>>;
        fn integer_node_get_value(node: &UniquePtr<CIntegerParameter>) -> Result<i64>;
        fn integer_node_get_min(node: &UniquePtr<CIntegerParameter>) -> Result<i64>;
        fn integer_node_get_max(node: &UniquePtr<CIntegerParameter>) -> Result<i64>;
        fn integer_node_set_value(node: &UniquePtr<CIntegerParameter>, value: i64) -> Result<()>;

        fn float_node_get_unit(node: &UniquePtr<CFloatParameter>) -> Result<UniquePtr<CxxString>>;
        fn float_node_get_value(node: &UniquePtr<CFloatParameter>) -> Result<f64>;
        fn float_node_get_min(node: &UniquePtr<CFloatParameter>) -> Result<f64>;
        fn float_node_get_max(node: &UniquePtr<CFloatParameter>) -> Result<f64>;
        fn float_node_set_value(node: &UniquePtr<CFloatParameter>, value: f64) -> Result<()>;

        fn enum_node_get_value(node: &UniquePtr<CEnumParameter>) -> Result<UniquePtr<CxxString>>;
        fn enum_node_settable_values(
            enum_node: &UniquePtr<CEnumParameter>,
        ) -> Result<UniquePtr<CxxVector<CxxString>>>;
        fn enum_node_set_value(enum_node: &UniquePtr<CEnumParameter>, value: &str) -> Result<()>;

        fn new_grab_result_ptr() -> Result<UniquePtr<CGrabResultPtr>>;
        fn grab_result_grab_succeeded(grab_result: &UniquePtr<CGrabResultPtr>) -> Result<bool>;
        fn grab_result_error_description(grab_result: &UniquePtr<CGrabResultPtr>)
            -> Result<String>;
        fn grab_result_error_code(grab_result: &UniquePtr<CGrabResultPtr>) -> Result<u32>;
        fn grab_result_width(grab_result: &UniquePtr<CGrabResultPtr>) -> Result<u32>;
        fn grab_result_height(grab_result: &UniquePtr<CGrabResultPtr>) -> Result<u32>;
        fn grab_result_offset_x(grab_result: &UniquePtr<CGrabResultPtr>) -> Result<u32>;
        fn grab_result_offset_y(grab_result: &UniquePtr<CGrabResultPtr>) -> Result<u32>;
        fn grab_result_padding_x(grab_result: &UniquePtr<CGrabResultPtr>) -> Result<u32>;
        fn grab_result_padding_y(grab_result: &UniquePtr<CGrabResultPtr>) -> Result<u32>;
        fn grab_result_buffer(grab_result: &UniquePtr<CGrabResultPtr>) -> Result<&[u8]>;
        fn grab_result_payload_size(grab_result: &UniquePtr<CGrabResultPtr>) -> Result<u32>;
        fn grab_result_buffer_size(grab_result: &UniquePtr<CGrabResultPtr>) -> Result<u32>;
        fn grab_result_block_id(grab_result: &UniquePtr<CGrabResultPtr>) -> Result<u64>;
        fn grab_result_time_stamp(grab_result: &UniquePtr<CGrabResultPtr>) -> Result<u64>;
        fn grab_result_stride(grab_result: &UniquePtr<CGrabResultPtr>) -> Result<usize>;
        fn grab_result_image_size(grab_result: &UniquePtr<CGrabResultPtr>) -> Result<u32>;

        fn device_info_copy(device_info: &CDeviceInfo) -> UniquePtr<CDeviceInfo>;
        fn device_info_get_property_names(
            device_info: &UniquePtr<CDeviceInfo>,
        ) -> Result<UniquePtr<CxxVector<CxxString>>>;
        fn device_info_get_property_value(
            device_info: &UniquePtr<CDeviceInfo>,
            name: &str,
        ) -> Result<String>;
        fn device_info_get_model_name(device_info: &UniquePtr<CDeviceInfo>) -> Result<String>;
    }
}
pub use ffi::TimeoutHandling;

pub struct PylonAutoInit {}

impl PylonAutoInit {
    pub fn new() -> Self {
        ffi::PylonInitialize();
        // println!("pylon initialized");
        Self {}
    }
}

impl Drop for PylonAutoInit {
    fn drop(&mut self) {
        ffi::PylonTerminate(true);
        // println!("pylon terminated");
    }
}

/// Wrap the CTlFactory type
// Since in C++ `CTlFactory::GetInstance()` merely returns a reference to
// a static object, here we don't store anything and instead get the
// reference when needed.
pub struct TlFactory {}

impl TlFactory {
    pub fn instance() -> Self {
        Self {}
    }
    pub fn create_first_device(&self) -> PylonResult<InstantCamera> {
        let inner = ffi::tl_factory_create_first_device()?;
        Ok(InstantCamera { inner })
    }
    pub fn create_device(&self, device_info: &DeviceInfo) -> PylonResult<InstantCamera> {
        let inner = ffi::tl_factory_create_device(&device_info.inner)?;
        Ok(InstantCamera { inner })
    }
    pub fn enumerate_devices(&self) -> PylonResult<Vec<DeviceInfo>> {
        let devs: cxx::UniquePtr<cxx::CxxVector<ffi::CDeviceInfo>> =
            ffi::tl_factory_enumerate_devices()?;
        Ok(devs
            .into_iter()
            .map(|cdev: &ffi::CDeviceInfo| DeviceInfo {
                inner: ffi::device_info_copy(cdev),
            })
            .collect())
    }
}

/// Wrap the CInstantCamera type
pub struct InstantCamera {
    inner: cxx::UniquePtr<ffi::CInstantCamera>,
}

/// Options passed to `start_grabbing`.
pub struct GrabOptions {
    count: Option<u32>,
}

impl Default for GrabOptions {
    fn default() -> Self {
        Self { count: None }
    }
}

impl GrabOptions {
    pub fn count(self, count: u32) -> GrabOptions {
        Self {
            count: Some(count),
            ..self
        }
    }
}

pub struct BooleanNode {
    inner: cxx::UniquePtr<ffi::CBooleanParameter>,
}

impl BooleanNode {
    pub fn value(&self) -> PylonResult<bool> {
        ffi::boolean_node_get_value(&self.inner).to_rust()
    }

    pub fn set_value(&mut self, value: bool) -> PylonResult<()> {
        ffi::boolean_node_set_value(&self.inner, value).to_rust()
    }
}

pub struct IntegerNode {
    inner: cxx::UniquePtr<ffi::CIntegerParameter>,
}

impl IntegerNode {
    pub fn unit(&self) -> PylonResult<String> {
        let cstr = ffi::integer_node_get_unit(&self.inner)?;
        Ok(cstr.to_str()?.to_string())
    }

    pub fn value(&self) -> PylonResult<i64> {
        ffi::integer_node_get_value(&self.inner).to_rust()
    }

    pub fn min(&self) -> PylonResult<i64> {
        ffi::integer_node_get_min(&self.inner).to_rust()
    }

    pub fn max(&self) -> PylonResult<i64> {
        ffi::integer_node_get_max(&self.inner).to_rust()
    }

    pub fn set_value(&mut self, value: i64) -> PylonResult<()> {
        ffi::integer_node_set_value(&self.inner, value).to_rust()
    }
}

pub struct FloatNode {
    inner: cxx::UniquePtr<ffi::CFloatParameter>,
}

impl FloatNode {
    pub fn unit(&self) -> PylonResult<String> {
        let cstr = ffi::float_node_get_unit(&self.inner)?;
        Ok(cstr.to_str()?.to_string())
    }

    pub fn value(&self) -> PylonResult<f64> {
        ffi::float_node_get_value(&self.inner).to_rust()
    }

    pub fn min(&self) -> PylonResult<f64> {
        ffi::float_node_get_min(&self.inner).to_rust()
    }

    pub fn max(&self) -> PylonResult<f64> {
        ffi::float_node_get_max(&self.inner).to_rust()
    }

    pub fn set_value(&mut self, value: f64) -> PylonResult<()> {
        ffi::float_node_set_value(&self.inner, value).to_rust()
    }
}

pub struct EnumNode {
    inner: cxx::UniquePtr<ffi::CEnumParameter>,
}

impl EnumNode {
    pub fn value(&self) -> PylonResult<String> {
        let cstr = ffi::enum_node_get_value(&self.inner)?;
        Ok(cstr.to_str()?.to_string())
    }
    pub fn settable_values(&self) -> PylonResult<Vec<String>> {
        ffi::enum_node_settable_values(&self.inner)?.to_rust()
    }
    pub fn set_value(&mut self, value: &str) -> PylonResult<()> {
        ffi::enum_node_set_value(&self.inner, value).to_rust()
    }
}

pub trait NodeMap {
    fn boolean_node(&self, name: &str) -> PylonResult<BooleanNode>;
    fn integer_node(&self, name: &str) -> PylonResult<IntegerNode>;
    fn float_node(&self, name: &str) -> PylonResult<FloatNode>;
    fn enum_node(&self, name: &str) -> PylonResult<EnumNode>;
}

unsafe impl Send for InstantCamera {}

impl NodeMap for InstantCamera {
    fn boolean_node(&self, name: &str) -> PylonResult<BooleanNode> {
        let inner = ffi::node_map_get_boolean_parameter(&self.inner, name)?;
        Ok(BooleanNode { inner })
    }
    fn integer_node(&self, name: &str) -> PylonResult<IntegerNode> {
        let inner = ffi::node_map_get_integer_parameter(&self.inner, name)?;
        Ok(IntegerNode { inner })
    }
    fn float_node(&self, name: &str) -> PylonResult<FloatNode> {
        let inner = ffi::node_map_get_float_parameter(&self.inner, name)?;
        Ok(FloatNode { inner })
    }
    fn enum_node(&self, name: &str) -> PylonResult<EnumNode> {
        let inner = ffi::node_map_get_enum_parameter(&self.inner, name)?;
        Ok(EnumNode { inner })
    }
}

impl InstantCamera {
    pub fn device_info(&self) -> DeviceInfo {
        // According to InstantCamera.h, `GetDeviceInfo()` does not throw C++ exceptions.
        let di = ffi::instant_camera_get_device_info(&self.inner);
        DeviceInfo { inner: di }
    }

    pub fn open(&self) -> PylonResult<()> {
        ffi::instant_camera_open(&self.inner).to_rust()
    }

    pub fn is_open(&self) -> PylonResult<bool> {
        ffi::instant_camera_is_open(&self.inner).to_rust()
    }

    pub fn close(&self) -> PylonResult<()> {
        ffi::instant_camera_close(&self.inner).to_rust()
    }

    pub fn start_grabbing(&self, options: &GrabOptions) -> PylonResult<()> {
        match options.count {
            None => ffi::instant_camera_start_grabbing(&self.inner).to_rust(),
            Some(count) => {
                ffi::instant_camera_start_grabbing_with_count(&self.inner, count).to_rust()
            }
        }
    }

    pub fn stop_grabbing(&self) -> PylonResult<()> {
        ffi::instant_camera_stop_grabbing(&self.inner).to_rust()
    }

    pub fn is_grabbing(&self) -> bool {
        // According to InstantCamera.h, `IsGrabbing()` does not throw C++ exceptions.
        ffi::instant_camera_is_grabbing(&self.inner)
    }

    pub fn retrieve_result(
        &self,
        timeout_ms: u32,
        grab_result: &mut GrabResult,
        timeout_handling: TimeoutHandling,
    ) -> PylonResult<bool> {
        ffi::instant_camera_retrieve_result(
            &self.inner,
            timeout_ms,
            &mut grab_result.inner,
            timeout_handling,
        )
        .to_rust()
    }
}

pub struct GrabResult {
    inner: cxx::UniquePtr<ffi::CGrabResultPtr>,
}

unsafe impl Send for GrabResult {}

impl GrabResult {
    pub fn new() -> PylonResult<Self> {
        Ok(Self {
            inner: ffi::new_grab_result_ptr()?,
        })
    }

    pub fn grab_succeeded(&self) -> PylonResult<bool> {
        ffi::grab_result_grab_succeeded(&self.inner).to_rust()
    }

    pub fn error_description(&self) -> PylonResult<String> {
        ffi::grab_result_error_description(&self.inner).to_rust()
    }

    pub fn error_code(&self) -> PylonResult<u32> {
        ffi::grab_result_error_code(&self.inner).to_rust()
    }

    pub fn width(&self) -> PylonResult<u32> {
        ffi::grab_result_width(&self.inner).to_rust()
    }

    pub fn height(&self) -> PylonResult<u32> {
        ffi::grab_result_height(&self.inner).to_rust()
    }

    pub fn offset_x(&self) -> PylonResult<u32> {
        ffi::grab_result_offset_x(&self.inner).to_rust()
    }

    pub fn offset_y(&self) -> PylonResult<u32> {
        ffi::grab_result_offset_y(&self.inner).to_rust()
    }

    pub fn padding_x(&self) -> PylonResult<u32> {
        ffi::grab_result_padding_x(&self.inner).to_rust()
    }

    pub fn padding_y(&self) -> PylonResult<u32> {
        ffi::grab_result_padding_y(&self.inner).to_rust()
    }

    pub fn buffer(&self) -> PylonResult<&[u8]> {
        ffi::grab_result_buffer(&self.inner).to_rust()
    }

    pub fn block_id(&self) -> PylonResult<u64> {
        ffi::grab_result_block_id(&self.inner).to_rust()
    }

    pub fn time_stamp(&self) -> PylonResult<u64> {
        ffi::grab_result_time_stamp(&self.inner).to_rust()
    }

    pub fn stride(&self) -> PylonResult<usize> {
        // ffi::grab_result_stride(&self.inner).to_rust()
        ffi::grab_result_stride(&self.inner).to_rust()
    }
}

trait CxxResultExt {
    type RustResult;
    fn to_rust(self) -> Self::RustResult;
}

impl CxxResultExt for cxx::UniquePtr<cxx::CxxVector<cxx::CxxString>> {
    type RustResult = PylonResult<Vec<String>>;
    fn to_rust(self) -> Self::RustResult {
        // This needs to return a Result (and cannot move the data, but rather
        // copy) because we need to ensure the strings are correct UTF8.
        Ok(self
            .into_iter()
            .map(|name| name.to_str().map(String::from))
            .collect::<Result<_, std::str::Utf8Error>>()?)
    }
}

impl<T> CxxResultExt for Result<T, cxx::Exception> {
    type RustResult = PylonResult<T>;
    fn to_rust(self) -> Self::RustResult {
        self.map_err(PylonError::from)
    }
}

// ---------------------------
// HasProperties trait

pub trait HasProperties {
    fn property_names(&self) -> PylonResult<Vec<String>>;
    fn property_value(&self, name: &str) -> PylonResult<String>;
}

impl HasProperties for DeviceInfo {
    fn property_names(&self) -> PylonResult<Vec<String>> {
        ffi::device_info_get_property_names(&self.inner)?.to_rust()
    }

    fn property_value(&self, name: &str) -> PylonResult<String> {
        Ok(ffi::device_info_get_property_value(&self.inner, name)?)
    }
}

impl DeviceInfo {
    pub fn model_name(&self) -> PylonResult<String> {
        ffi::device_info_get_model_name(&self.inner).to_rust()
    }
}

pub struct DeviceInfo {
    inner: cxx::UniquePtr<ffi::CDeviceInfo>,
}

impl Clone for DeviceInfo {
    fn clone(&self) -> DeviceInfo {
        DeviceInfo {
            inner: ffi::device_info_copy(&self.inner),
        }
    }
}

unsafe impl Send for DeviceInfo {}
