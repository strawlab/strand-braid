import pyci2._pyci2 as _pyci2
import numpy as np

ci2_module = None

def load_ci2():
    global ci2_module
    if ci2_module is not None:
        raise RuntimeError("ci2_module already loaded")
    ci2_module = _pyci2.CameraModule()

load_ci2()

class CamIFaceError(Exception):
    pass

class FrameDataMissing(CamIFaceError):
    pass

class FrameDataCorrupt(CamIFaceError):
    pass

class FrameSystemCallInterruption(CamIFaceError):
    pass

class BuffersOverflowed(CamIFaceError):
    pass

class NoFrameReturned(CamIFaceError):
    pass

class CameraNotAvailable(CamIFaceError):
    pass

class Camera(object):
    def __init__(self,cam_no,num_buffers,mode_num):
        self.pixel_coding = ci2_module.init_camera(cam_no)
        assert mode_num==0
        self.cam_no = cam_no

    def start_camera(self):
        ci2_module.start_camera(self.cam_no)

    def get_frame_roi(self):
        return ci2_module.get_frame_roi(self.cam_no)

    def get_pixel_depth(self):
        return 8

    def get_max_width(self):
        _,_,w,h = ci2_module.get_frame_roi(self.cam_no)
        return w

    def get_max_height(self):
        _,_,w,h = ci2_module.get_frame_roi(self.cam_no)
        return h

    def get_framerate(self):
        return 12.34

    def get_num_framebuffers(self):
        return 123

    def set_frame_roi(self,l,t,w,h):
        cur = ci2_module.get_frame_roi(self.cam_no)
        if (l,t,w,h) != cur:
            raise NotImplementedError()

    def grab_next_frame_blocking(self):
        _,_,w,h = ci2_module.get_frame_roi(self.cam_no)
        arr = np.empty((h,w), dtype=np.uint8)
        tstamp, fno = ci2_module.grab_next_frame_blocking(self.cam_no, arr)
        self.tstamp = tstamp
        self.fno = fno
        return arr

    def grab_next_frame_into_alloced_buf_blocking(self,buf_alloc):
        l,t,w,h = ci2_module.get_frame_roi(self.cam_no)
        buf = buf_alloc(w,h)
        self.grab_next_frame_into_buf_blocking(buf)
        return buf

    def grab_next_frame_into_buf_blocking(self,arr):
        npview = np.array(arr,copy=False) # create a view of the data in arr
        tstamp, fno = ci2_module.grab_next_frame_blocking(self.cam_no, npview)
        self.tstamp = tstamp
        self.fno = fno
        return arr

    def get_pixel_coding(self):
        return self.pixel_coding

    def get_num_camera_properties(self):
        return 2

    def get_camera_property_info(self,i):
        if i==0:
            return {
                'name': "shutter",
                'min_value': 1,
                'max_value': 1000000,
                'has_manual_mode': True,
                'has_auto_mode': True,
                'is_present': True,
                'is_scaled_quantity': False,
                }
        elif i==1:
            return {'name': "gain",
                'min_value': 300,
                'max_value': 100300,
                'has_manual_mode': True,
                'has_auto_mode': True,
                'is_present': True,
                'is_scaled_quantity': False,
                }
        else:
            raise ValueError("")

    def set_camera_property(self,prop_num,prop_val,auto):
        ci2_module.set_camera_property(self.cam_no,prop_num,prop_val,auto)

    def get_camera_property(self,prop_num):
        return ci2_module.get_camera_property(self.cam_no,prop_num)

    def get_num_trigger_modes(self):
        return 2

    def get_trigger_mode_string(self,i):
        if i==0:
            return "freerunning"
        elif i==1:
            return "external trigger"
        else:
            raise ValueError()

    def get_trigger_mode_number(self):
        if ci2_module.get_external_trig(self.cam_no):
            return 1
        else:
            return 0

    def set_framerate(self, x):
        raise NotImplementedError("")

    def set_trigger_mode_number(self,x):
        raise NotImplementedError("")

    def get_last_timestamp(self):
        return self.tstamp

    def get_last_framenumber(self):
        return self.fno

def get_num_cameras():
    return ci2_module.get_num_cameras()

def get_num_modes(cam_no):
    return 1

def get_camera_info(cam_no):
    vendor, model, serial, name = ci2_module.get_camera_info(cam_no)
    uuid = "%s-%s" % (vendor,serial)
    return vendor, model, uuid

def get_mode_string(cam_no,mode_num):
    assert mode_num==0
    return "default"

def get_driver_name():
    return "ci2"

def get_wrapper_name():
    return "pyci2"
