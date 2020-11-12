# Pure Python definition of FrameData class.
#
# Note: we do not want to import the native library here to
# keep this pure Python.


class PixelFormat(object):
    pass


MONO8 = PixelFormat()
BayerRG8 = PixelFormat()


class FrameData(object):
    def __init__(self, buf, pixel_format, shape):
        self._buf = buf
        self._pixel_format = pixel_format
        self._shape = shape

    def buffer_view(self):
        return self._buf

    def shape(self):
        return self._shape

    def pixel_format(self):
        return self._pixel_format
