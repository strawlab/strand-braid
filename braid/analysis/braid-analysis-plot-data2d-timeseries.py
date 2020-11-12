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
    cam_info_df = pd.read_csv(
        archive.open('cam_info.csv.gz'),
        comment="#",
        compression='gzip')

    camn2camid = {}
    for i, row in cam_info_df.iterrows():
        camn2camid[row['camn']] = row['cam_id']

    cam_ids = list(cam_info_df['cam_id'].values)
    cam_ids.sort()
    data2d_distorted_df = pd.read_csv(
        archive.open('data2d_distorted.csv.gz'),
        comment="#",
        compression='gzip')

fig, axes = plt.subplots(nrows=len(cam_ids),sharex=True, squeeze=False)
assert axes.shape[1]==1
axes = axes[:,0]

for camn, gdf in data2d_distorted_df.groupby('camn'):
    camid = camn2camid[camn]
    idx = cam_ids.index(camid)
    ax = axes[idx]

    cam_id_n_valid = np.sum(~np.isnan(gdf['x']))

    ax.plot(gdf['frame'], gdf['x'], 'r.', ms=0.3, label='x')
    ax.plot(gdf['frame'], gdf['y'], 'g.', ms=0.3, label='y')

    ax.text(0.1,0,'%s %s: %d pts'%(camid,camn,cam_id_n_valid),
            horizontalalignment='left',
            verticalalignment='bottom',
            transform = ax.transAxes,
            )
    ax.set_ylabel('pixel')

axes[-1].set_xlabel('frame')
axes[0].legend(loc='upper right', numpoints=5, markerscale=10)

plt.show()
