#!/usr/bin/env python
import sys
import zipfile
import urllib.request # requires Python 3
import io
import pandas as pd
import numpy as np
import matplotlib.pyplot as plt

def open_filename_or_url(filename_or_url):
    parsed = urllib.parse.urlparse(filename_or_url)
    if parsed.scheme=='':
        # no scheme, so this is a filename.
        fileobj_with_seek = open(filename_or_url,mode='rb')
    else:
        # Idea for one day: implement HTTP file object reader that implements
        # seek using HTTP range requests.
        fileobj = urllib.request.urlopen(filename_or_url)
        fileobj_with_seek = io.BytesIO(fileobj.read())
    return fileobj_with_seek

filename_or_url = sys.argv[1]

fileobj = open_filename_or_url(filename_or_url)

with zipfile.ZipFile(file=fileobj, mode='r') as archive:
    df = pd.read_csv(
        archive.open('kalman_estimates.csv.gz'),
        comment="#",
        compression='gzip')

dimnames = ['x','y','z']
fig, axes = plt.subplots(nrows=len(dimnames),sharex=True)

for ax,dimname in zip(axes,dimnames):
    for obj_id, gdf in df.groupby('obj_id'):
        ax.plot(gdf['frame'], gdf[dimname], '-', label=str(obj_id))
    ax.set_ylabel(dimname)

axes[-1].set_xlabel('frame')
axes[0].legend(loc='upper right')

plt.show()
