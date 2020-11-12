import strandcam.frame_annotation
import strandcam.app_runner
import numpy as np

def myfunc(frame_data, timestamp):
    """handle a new frame each time one is ready

    Arguments
    =========
    frame_data - an instance of strandcam.frame_data.FrameData
    timestamp - the time in seconds since the epoch as a floating point number

    Note: This will be called from a different thread than the main loop.
    """

    # Convert incoming raw buffer view into a numpy array
    arr = np.frombuffer(frame_data.buffer_view(), dtype=np.uint8)
    arr.shape = frame_data.shape()

    # Find the brightest point using numpy
    (ymax, xmax) = np.unravel_index(np.argmax(arr, axis=None), arr.shape)

    # Return a sequence of points to draw
    points = (
        strandcam.frame_annotation.Point(xmax, ymax),
    )
    return strandcam.frame_annotation.FrameAnnotation(points)

# Run the main loop. This will print a URL with the browser UI.
strandcam.app_runner.run_forever(process_frame=myfunc)
