#!/usr/bin/env python
from __future__ import print_function
import sys
import os
import argparse
import zipfile
import numpy as np

if sys.version_info.major < 3:
    import urlparse as parsemod
    import urllib as openmod
else:
    import urllib.parse as parsemod
    import urllib.request as openmod

import hdrh # On Ubuntu 16 with Python 2.x: pip install hdrhistogram==0.8.0
from hdrh.histogram import HdrHistogram
from hdrh.log import HistogramLogReader

# Remove this if https://github.com/HdrHistogram/HdrHistogram_py/issues/28
# is resolved.
class HistogramLogObjReader(HistogramLogReader):
    def __init__(self, input_file, reference_histogram):
        '''Constructs a new HistogramLogReader that produces intervals read
        from the specified file name.
        Params:
            input_file The file to read from
            reference_histogram a histogram instance used as a reference to create
                                new instances for all subsequent decoded interval
                                histograms
        '''
        self.start_time_sec = 0.0
        self.observed_start_time = False
        self.base_time_sec = 0.0
        self.observed_base_time = False
        self.input_file = input_file
        self.reference_histogram = reference_histogram

def open_filename_or_url(filename_or_url):
    parsed = parsemod.urlparse(filename_or_url)
    if parsed.scheme=='':
        # no scheme, so this is a filename.
        fileobj_with_seek = open(filename_or_url,mode='rb')
    else:
        # Idea for one day: implement HTTP file object reader that implements
        # seek using HTTP range requests.
        fileobj = openmod.urlopen(filename_or_url)
        fileobj_with_seek = io.BytesIO(fileobj.read())
    return fileobj_with_seek

def show_hist(fd, title, scale):
    LOWEST = 1
    HIGHEST = 10000000
    SIGNIFICANT = 2
    accumulated_histogram = hdrh.histogram.HdrHistogram(LOWEST, HIGHEST, SIGNIFICANT)
    log_reader = HistogramLogObjReader(fd, accumulated_histogram)

    while log_reader.add_next_interval_histogram() is not None:
        pass
    print('{} ---------------------'.format(title))
    accumulated_histogram.output_percentile_distribution(sys.stdout, scale, ticks_per_half_distance=1)
    print()

parser = argparse.ArgumentParser()
parser.add_argument("filename_or_url", type=str, help="input file or URL, .braidz or .unconverted.zip", nargs=1)
args = parser.parse_args()
filename_or_url = args.filename_or_url[0]
fileobj = open_filename_or_url(filename_or_url)

with zipfile.ZipFile(file=fileobj, mode='r') as archive:
    reproj_hist_fd = archive.open('reprojection_distance_100x_pixels.hlog')
    show_hist(reproj_hist_fd, 'Reprojection Distance (Pixels)', 100.0)

    reproj_hist_fd = archive.open('reconstruct_latency_usec.hlog')
    show_hist(reproj_hist_fd, '3D reconstruction latency (msec)', 1000.0)
