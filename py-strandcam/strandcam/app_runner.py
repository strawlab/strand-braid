from strandcam._native import ffi, lib
import strandcam.frame_data
import strandcam.frame_annotation


class StrandCamError(Exception):
    pass


# Can register specific error subclasses for codes
exceptions_by_code = {}


def _fficall(func, *args):
    """Calls FFI method and does some error handling."""
    lib.strandcam_err_clear()
    rv = func(*args)
    err = lib.strandcam_err_get_last_code()
    if not err:
        return rv
    msg = lib.strandcam_err_get_last_message()
    cls = exceptions_by_code.get(err, StrandCamError)
    raise cls(decode_str(msg))


def decode_str(s, free=False):
    """Decodes a EisvogelStr"""
    try:
        if s.len == 0:
            return u""
        return ffi.unpack(s.data, s.len).decode("utf-8", "replace")
    finally:
        if free:
            lib.strandcam_str_free(ffi.addressof(s))


def _convert_pixel_format(c_pix_fmt):
    if c_pix_fmt == lib.MONO8:
        return strandcam.frame_data.MONO8
    elif c_pix_fmt == lib.BayerRG8:
        return strandcam.frame_data.BayerRG8
    else:
        raise ValueError("unknown or unsupported C pixel format: %s" % c_pix_fmt)


def _convert_result(py_result):
    assert isinstance(py_result, strandcam.frame_annotation.FrameAnnotation)
    n_points = len(py_result._points)
    c_result = lib.strandcam_new_frame_annotation_zeros(n_points)
    c_ptr = ffi.addressof(c_result)
    for (i, pt) in enumerate(py_result._points):
        lib.strandcam_set_frame_annotation(c_ptr, i, pt.x, pt.y)
    return c_result


@ffi.callback("StrandCamFrameAnnotation(FrameData *, void *, double)")
def _global_process_frame_cb(frame_data, handle, timestamp):
    size = frame_data.stride * frame_data.rows
    buf = ffi.buffer(frame_data.data, size)
    py_result = ffi.from_handle(handle).do_python_callback(
        buf, frame_data.pixel_format, (frame_data.rows, frame_data.cols),
        timestamp,
    )
    c_result = _convert_result(py_result)
    return c_result


class _AppRunner(object):
    def __init__(self, process_frame=None):
        self.process_frame = process_frame
        handle = ffi.new_handle(self)
        self._handle = handle  # must be kept alive
        _fficall(lib.sc_run_app_with_process_frame_cb, _global_process_frame_cb, handle)

    def do_python_callback(self, buf, pixel_format, shape, timestamp):
        pypixfmt = _convert_pixel_format(pixel_format)
        wrapped = strandcam.frame_data.FrameData(buf, pypixfmt, shape)
        result = self.process_frame(
            wrapped,
            timestamp,
        )  # warning: all references to buffer must be released upon return.
        # TODO handle return value
        return result


def run_forever(process_frame=None):
    app_runner = _AppRunner(process_frame)
