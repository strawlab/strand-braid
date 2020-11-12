#[macro_use]
extern crate cpython;
#[cfg(feature = "dc1394")]
extern crate ci2_dc1394;
#[cfg(feature = "flycap2")]
extern crate ci2_flycap2;
#[cfg(feature = "pylon")]
extern crate ci2_pylon;
extern crate ci2;
extern crate parking_lot;
#[macro_use]
extern crate lazy_static;
#[macro_use]
extern crate error_chain;
#[macro_use]
extern crate log;
extern crate machine_vision_formats as formats;

use std::sync::Arc;
use std::collections::HashMap;
use parking_lot::Mutex;
use std::sync::mpsc::{sync_channel, Receiver, SyncSender, RecvError};

use ci2::CameraModule as ci2_CameraModule;
use cpython::{Python, PyResult, PyErr, exc, PyObject};

lazy_static! {
    static ref IS_STARTED: Arc<Mutex<bool>> = {
        Arc::new(Mutex::new(false))
    };
}

#[cfg(feature = "dc1394")]
fn get_camera_module() -> ci2::Result<Box<ci2_dc1394::WrappedModule>> {
    ci2_dc1394::WrappedModule::new()
}

#[cfg(feature = "flycap2")]
fn get_camera_module() -> ci2::Result<Box<ci2_flycap2::WrappedModule>> {
    ci2_flycap2::WrappedModule::new()
}

#[cfg(feature = "pylon")]
fn get_camera_module() -> ci2::Result<Box<ci2_pylon::WrappedModule>> {
    ci2_pylon::WrappedModule::new()
}

enum ToCamThreadMsg {
    GetNumCameras,
    GetCameraInfo(u64),
    InitCamera(u64),
    StartCamera(u64),
    GrabNextFrameBlocking(u64),
    GetFrameROI(u64),
    GetTriggerInfo(u64),
    SetCameraProperty(u64, usize, i64, i32),
    GetCameraProperty(u64, usize),
}

struct CamInfo {
    vendor: String,
    model: String,
    serial: String,
    name: String,
}

enum FromCamThreadMsg {
    NoError,
    CamInited(String),
    CameraInfo(CamInfo),
    NumCameras(usize),
    Frame(formats::Frame),
    FrameROI(formats::FrameROI),
    TriggerInfo(ci2::TriggerMode, ci2::TriggerSelector, ci2::AcquisitionMode),
    CameraProperty(i64, bool),
}

fn cam_thread(from_cam_thread_tx: SyncSender<FromCamThreadMsg>,
              to_cam_thread_rx: Receiver<ToCamThreadMsg>)
              -> ci2::Result<()> {
    let mut mymod = get_camera_module()?;
    info!("camera module: {}", mymod.name());

    let cam_infos = mymod.camera_infos()?;
    if cam_infos.len()==0 {
        bail!("No cameras found.")
    }

    let mut cameras = HashMap::new();

    loop {
        match to_cam_thread_rx.recv() {
            Ok(msg) => {
                match msg {
                    ToCamThreadMsg::GetNumCameras => {
                        from_cam_thread_tx
                            .send( FromCamThreadMsg::NumCameras(cam_infos.len()) )
                            .expect("sending NumCameras");
                    },
                    ToCamThreadMsg::GetCameraInfo(cam_no) => {
                        let oci = cam_infos.get(cam_no as usize).unwrap();
                        let ci = CamInfo {
                            vendor: oci.vendor().into(),
                            model: oci.model().into(),
                            serial: oci.serial().into(),
                            name: oci.name().into(),
                        };
                        from_cam_thread_tx
                            .send( FromCamThreadMsg::CameraInfo(ci) )
                            .expect("sending CameraInfo");
                    },
                    ToCamThreadMsg::InitCamera(cam_no) => {
                        if let Some(info) = cam_infos.get(cam_no as usize) {
                            let mut cam = mymod.camera(info.name())?;
                            cam.set_acquisition_mode(ci2::AcquisitionMode::Continuous)?;
                            let pixel_format = cam.pixel_format()?;
                            let fmt = match pixel_format {
                                formats::PixelFormat::MONO8 => "MONO8",
                                formats::PixelFormat::BayerRG8 => "RAW8:RGGB",
                                formats::PixelFormat::BayerBG8 => "RAW8:BGGR",
                                formats::PixelFormat::BayerGB8 => "RAW8:GBRG",
                                formats::PixelFormat::BayerGR8 => "RAW8:GRBG",
                                e => {
                                    error!("unimplemented pixel_format {:?}", e);
                                    unimplemented!();
                                },
                            };
                            cameras.insert(cam_no, cam);
                            from_cam_thread_tx
                                .send(FromCamThreadMsg::CamInited(fmt.to_string()))
                                .expect("sending CamInited");
                        } else {
                            bail!("only {} cams, no cam_no {}", cam_infos.len(), cam_no);
                        }
                    },
                    ToCamThreadMsg::StartCamera(cam_no) => {
                        match cameras.get_mut(&cam_no) {
                            Some(cam) => {
                                cam.acquisition_start()?;
                                from_cam_thread_tx.send( FromCamThreadMsg::NoError ).expect("sending NoError");
                            },
                            None => bail!("cam_no {} unknown", cam_no),
                        };
                    },
                    ToCamThreadMsg::GrabNextFrameBlocking(cam_no) => {
                        match cameras.get_mut(&cam_no) {
                            Some(cam) => {
                                let frame = cam.next_frame(None)?;
                                from_cam_thread_tx.send( FromCamThreadMsg::Frame(frame) ).expect("sending Frame");
                            },
                            None => bail!("cam_no {} unknown", cam_no),
                        };
                    },
                    ToCamThreadMsg::GetFrameROI(cam_no) => {
                        match cameras.get_mut(&cam_no) {
                            Some(cam) => {
                                let roi = cam.roi()?;
                                from_cam_thread_tx.send(
                                    FromCamThreadMsg::FrameROI(roi)).expect("sending FrameROI");
                            },
                            None => bail!("cam_no {} unknown", cam_no),
                        };
                    },
                    ToCamThreadMsg::GetTriggerInfo(cam_no) => {
                        match cameras.get_mut(&cam_no) {
                            Some(cam) => {
                                let trigger_mode = cam.trigger_mode()?;
                                let trigger_selector = cam.trigger_selector()?;
                                let acquisition_mode = cam.acquisition_mode()?;
                                from_cam_thread_tx.send(
                                    FromCamThreadMsg::TriggerInfo(
                                        trigger_mode, trigger_selector, acquisition_mode))
                                    .expect("sending TriggerInfo");
                            },
                            None => bail!("cam_no {} unknown", cam_no),
                        };
                    },
                    ToCamThreadMsg::SetCameraProperty(cam_no, prop_num, value, auto) => {
                        match cameras.get_mut(&cam_no) {
                            Some(cam) => {
                                match prop_num {
                                    0 => {
                                        // shutter
                                        cam.set_exposure_time( value as f64 )?; // value in usec
                                        if auto > 0 {
                                            cam.set_exposure_auto(ci2::AutoMode::Continuous)?;
                                        } else {
                                            cam.set_exposure_auto(ci2::AutoMode::Off)?;
                                        }
                                    },
                                    1 => {
                                        // gain
                                        cam.set_gain((value as f64 - 300.0) / 1000.0 )?; // convert to dB, 300 = 0dB
                                        if auto > 0 {
                                            cam.set_gain_auto(ci2::AutoMode::Continuous)?;
                                        } else {
                                            cam.set_gain_auto(ci2::AutoMode::Off)?;
                                        }
                                    },
                                    n => {
                                        bail!("prop_num {} unknown", n);
                                    },
                                }
                            },
                            None => bail!("cam_no {} unknown", cam_no),
                        };
                        from_cam_thread_tx.send( FromCamThreadMsg::NoError ).expect("sending NoError");
                    }
                    ToCamThreadMsg::GetCameraProperty(cam_no, prop_num) => {
                        match cameras.get_mut(&cam_no) {
                            Some(cam) => {
                                let (value, auto) = match prop_num {
                                    0 => { (cam.exposure_time()? as i64, cam.exposure_auto()?==ci2::AutoMode::Continuous) },
                                    1 => { ((cam.gain()? * 1000.0 + 300.0) as i64, cam.gain_auto()?==ci2::AutoMode::Continuous) },
                                    n => {
                                        bail!("prop_num {} unknown", n);
                                    },
                                };
                                from_cam_thread_tx.send( FromCamThreadMsg::CameraProperty(value,auto) ).expect("sending CameraProperty");
                            },
                            None => bail!("cam_no {} unknown", cam_no),
                        };
                    },
                }
            },
            Err(RecvError) => {
                debug!("RecvError in cam thread. quitting cam thread.");
                break;
            }
        }
    }
    Ok(())
}

fn recv_err_to_py_err(py: Python, _orig: RecvError) -> PyErr {
    let msg = format!("RecvError: {:?}", _orig);
    PyErr::new::<exc::TypeError, _>(py, msg)
}

py_class!(class CameraModule |py| {
    data from_cam_thread_rx: Receiver<FromCamThreadMsg>;
    data to_cam_thread_tx: SyncSender<ToCamThreadMsg>;
    def __new__(_cls) -> PyResult<CameraModule> {
        let mut is_started = (*IS_STARTED).lock();
        let (from_cam_thread_tx,from_cam_thread_rx) = sync_channel(0); // create rendevous channel
        let (to_cam_thread_tx,to_cam_thread_rx) = sync_channel(0); // create rendevous channel
        if !(*is_started) {

            std::thread::spawn(|| {
                match cam_thread(from_cam_thread_tx, to_cam_thread_rx) {
                    Ok(_) => {},
                    Err(e) => {
                        // TODO better error handling. Maybe send error to Python?
                        panic!("cam thread failed: {:?}", e);
                    }
                }
            });
            *is_started  = true;
        } else {
            return Err(PyErr::new::<exc::TypeError, _>(py, "CameraModule already started"));
        }
        CameraModule::create_instance(py, from_cam_thread_rx, to_cam_thread_tx)
    }
    def get_num_cameras(&self) -> PyResult<usize> {
        self.to_cam_thread_tx(py).send(ToCamThreadMsg::GetNumCameras).expect("sending GetNumCameras enum");
        match self.from_cam_thread_rx(py).recv().map_err(|e| recv_err_to_py_err(py,e))? {
            FromCamThreadMsg::NumCameras(n) => {
                Ok(n)
            },
            _ => {
                return Err(PyErr::new::<exc::TypeError, _>(py, "unexpected result from camera thread"));
            },
        }
    }
    def get_camera_info(&self, cam_no: u64) -> PyResult<(String,String,String,String)> {
        self.to_cam_thread_tx(py).send(ToCamThreadMsg::GetCameraInfo(cam_no)).expect("sending GetCameraInfo enum");
        match self.from_cam_thread_rx(py).recv().map_err(|e| recv_err_to_py_err(py,e))? {
            FromCamThreadMsg::CameraInfo(ci) => {
                Ok((ci.vendor,ci.model,ci.serial,ci.name))
            },
            _ => {
                return Err(PyErr::new::<exc::TypeError, _>(py, "unexpected result from camera thread"));
            },
        }
    }
    def init_camera(&self, cam_no: u64) -> PyResult<String> {
        debug!("init_camera called with cam_no={:?}", cam_no);
        self.to_cam_thread_tx(py).send(ToCamThreadMsg::InitCamera(cam_no)).expect("sending InitCamera enum");
        match self.from_cam_thread_rx(py).recv().map_err(|e| recv_err_to_py_err(py,e))? {
            FromCamThreadMsg::CamInited(pixel_coding) => {
                Ok(pixel_coding)
            },
            _ => {
                return Err(PyErr::new::<exc::TypeError, _>(py, "unexpected result from camera thread"));
            },
        }
    }
    def start_camera(&self, cam_no: u64) -> PyResult<PyObject> {
        debug!("start_camera called with cam_no={:?}", cam_no);
        self.to_cam_thread_tx(py).send(ToCamThreadMsg::StartCamera(cam_no)).expect("sending StartCamera enum");
        match self.from_cam_thread_rx(py).recv().map_err(|e| recv_err_to_py_err(py,e))? {
            FromCamThreadMsg::NoError => {
                Ok(py.None())
            },
            _ => {
                return Err(PyErr::new::<exc::TypeError, _>(py, "unexpected result from camera thread"));
            },
        }
    }
    def get_frame_roi(&self, cam_no: u64) -> PyResult<(u32,u32,u32,u32)> {
        debug!("get_frame_roi called with cam_no={:?}", cam_no);
        self.to_cam_thread_tx(py).send(ToCamThreadMsg::GetFrameROI(cam_no)).expect("sending GetFrameROI enum");
        match self.from_cam_thread_rx(py).recv().map_err(|e| recv_err_to_py_err(py,e))? {
            FromCamThreadMsg::FrameROI(roi) => {
                Ok((roi.xmin,roi.ymin,roi.width,roi.height))
            },
            _ => {
                return Err(PyErr::new::<exc::TypeError, _>(py, "unexpected result from camera thread"));
            },
        }
    }
    def set_camera_property(&self, cam_no: u64, prop_num: usize, value: i64, auto: i32) -> PyResult<PyObject> {
        self.to_cam_thread_tx(py).send(ToCamThreadMsg::SetCameraProperty(cam_no, prop_num, value, auto)).expect("sending SetCameraProperty enum");
        match self.from_cam_thread_rx(py).recv().map_err(|e| recv_err_to_py_err(py,e))? {
            FromCamThreadMsg::NoError => {
                Ok(py.None())
            },
            _ => {
                return Err(PyErr::new::<exc::TypeError, _>(py, "unexpected result from camera thread"));
            },
        }
    }
    def get_camera_property(&self, cam_no: u64, prop_num: usize) -> PyResult<(i64,bool)> {
        self.to_cam_thread_tx(py).send(ToCamThreadMsg::GetCameraProperty(cam_no, prop_num)).expect("sending GetCameraProperty enum");
        match self.from_cam_thread_rx(py).recv().map_err(|e| recv_err_to_py_err(py,e))? {
            FromCamThreadMsg::CameraProperty(val,auto) => {
                Ok((val,auto))
            },
            _ => {
                return Err(PyErr::new::<exc::TypeError, _>(py, "unexpected result from camera thread"));
            },
        }
    }
    def grab_next_frame_blocking(&self, cam_no: u64, buf_obj: PyObject) -> PyResult<(f64,usize)> {
        trace!("grab_next_frame_blocking called with cam_no={:?}", cam_no);
        self.to_cam_thread_tx(py).send(ToCamThreadMsg::GrabNextFrameBlocking(cam_no)).expect("sending GrabNextFrameBlocking enum");
        match self.from_cam_thread_rx(py).recv().map_err(|e| recv_err_to_py_err(py,e))? {
            FromCamThreadMsg::Frame(frame) => {
                if frame.roi.width != frame.stride {
                    error!("currently only support for frames with stride==width is implemented");
                    unimplemented!();
                }
                let buffer = cpython::buffer::PyBuffer::get(py, &buf_obj)?;
                assert_eq!(buffer.dimensions(), 2);
                assert_eq!(buffer.format().to_str().unwrap(), "B");
                assert!(buffer.is_c_contiguous());
                buffer.copy_from_slice(py, &frame.image_data)?;
                let dtl = frame.host_timestamp; // Datetime<Local>
                let dt = dtl.naive_utc();
                let secs = dt.timestamp();
                let nsecs = dt.timestamp_subsec_nanos();
                let timestamp: f64 = (secs as f64) + (nsecs as f64 * 1e-9);
                let fno = frame.host_framenumber; // usize
                Ok((timestamp,fno))
            },
            _ => {
                return Err(PyErr::new::<exc::TypeError, _>(py, "unexpected result from camera thread"));
            },
        }
    }
    def get_external_trig(&self, cam_no: u64) -> PyResult<bool> {
        debug!("get_external_trig called with cam_no={:?}", cam_no);
        self.to_cam_thread_tx(py).send(ToCamThreadMsg::GetTriggerInfo(cam_no)).expect("sending GetTriggerInfo enum");
        match self.from_cam_thread_rx(py).recv().map_err(|e| recv_err_to_py_err(py,e))? {
            FromCamThreadMsg::TriggerInfo(t,s,a) => {
                let result = match (t,s,a) {
                    (ci2::TriggerMode::On, ci2::TriggerSelector::FrameStart, ci2::AcquisitionMode::Continuous) => true,
                    (ci2::TriggerMode::Off, _, ci2::AcquisitionMode::Continuous) => false,
                    _ => {return Err(PyErr::new::<exc::TypeError, _>(py, "unsupported trigger configuration"))},//bail!("unsupported trigger configuration"),
                };
                Ok(result)
            },
            _ => {
                return Err(PyErr::new::<exc::TypeError, _>(py, "unexpected result from camera thread"));
            },
        }
    }
});

// add bindings to the generated python module
// N.B: names: "_pyci2" must be the name of the `.so` or `.pyd` file
py_module_initializer!(_pyci2, init_pyci2, PyInit_pyci2, |py, m| {
    try!(m.add(py, "__doc__", "This module is implemented in Rust."));
    try!(m.add_class::<CameraModule>(py));
    Ok(())
});
