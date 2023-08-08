"""Convert a flydra mainbrain HDF5 file to a .braid directory of CSV files.

This does the inverse of `convert_braidz_to_flydra_h5.py`. However,
it does not convert all data, just the subset sufficient to re-track.

The .braid directory can be converted to a .braidz file by zipping it.
"""
import sys
import os
import errno
import tables
import pandas
import imageio # On Ubuntu 16 with Python 2.x: pip install imageio==2.3 --no-deps
from collections import defaultdict
import flydra_analysis.a2.calibration_to_xml as calibration_to_xml

fname = sys.argv[1]
outdir, ext = os.path.splitext(fname)
assert ext == ".h5" or ext == ".hdf5"

images_dir = os.path.join(outdir, "images")

try:
    os.makedirs(images_dir) # also makes outdir
except OSError as err:
    if err.errno == errno.EEXIST:
        pass
    else:
        raise

with tables.open_file(fname) as h5:
    # 2d data ------
    d2d = h5.root.data2d_distorted[:]
    df = pandas.DataFrame(d2d)
    csv_fname = os.path.join(outdir, "data2d_distorted.csv")
    df.to_csv(csv_fname, index=False, float_format="%r")

    # textlog ------
    textlog = h5.root.textlog[:]
    df = pandas.DataFrame(textlog)
    df["cam_id"] = df["cam_id"].str.decode("ascii")
    df["message"] = df["message"].str.decode("ascii")
    csv_fname = os.path.join(outdir, "textlog.csv")
    df.to_csv(csv_fname, index=False, float_format="%r")

    # cam info ------
    cam_info = h5.root.cam_info[:]
    df = pandas.DataFrame(cam_info)
    df["cam_id"] = df["cam_id"].str.decode("ascii")
    csv_fname = os.path.join(outdir, "cam_info.csv")
    df.to_csv(csv_fname, index=False, float_format="%r")

    # images
    image_table = h5.root.images

    for row in h5.root.cam_info:
        cam_id = row["cam_id"]
        if sys.version_info.major >= 3:
            cam_id = str(cam_id, "utf-8")

        arr = getattr(image_table, cam_id)
        image = arr.read()
        if image.ndim == 3 and image.shape[2] == 1:
            # only a single "color" channel
            image = image[:, :, 0]  # drop a dimension (3D->2D)
        image_fname = os.path.join(images_dir, "%s.png" % (cam_id,))
        imageio.imsave(image_fname, image)

class Options:
    pass


options = Options()
options.scaled = False
options.dest = os.path.join(outdir, "calibration.xml")

calibration_to_xml.doit(fname, options)
