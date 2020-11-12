#!/usr/bin/env python3
"""print histograms of flydra timing data
"""

import json
import sys
import collections
import numpy as np
import pandas as pd
import datetime
from pytz import utc, timezone

fname = sys.argv[1]
count = 0
data = collections.defaultdict(list)
with open(fname,mode='r') as fd:
    for line in fd.readlines():
        row = json.loads(line.strip())
        if row['table_name'] != 'raw_packet':
            continue
        count += 1

        stamp = row['stamp']
        dot_idx = stamp.find('.')
        stamp2 = stamp[:dot_idx+7]
        stamp2 += 'Z'
        stamp_dt = datetime.datetime.strptime(stamp2, "%Y-%m-%dT%H:%M:%S.%fZ")
        stamp_f64 = stamp_dt.replace(tzinfo=utc).timestamp()

        val = row['val']

        if val['timestamp'] != 0.0:
            x1 = (val['cam_received_time'] - val['timestamp']) * 1000.0
        else:
            x1 = np.nan
        x2 = (val['preprocess_stamp'] - val['cam_received_time']) * 1000.0
        x3 = (val['done_camnode_processing'] - val['cam_received_time']) * 1000.0
        x4 = (stamp_f64 - val['cam_received_time']) * 1000.0

        bits = val['image_processing_steps']['bits']

        data['cam->computer'].append(x1)
        data['computer->improc'].append(x2)
        data['computer->send'].append(x3)
        data['computer->mainbrain'].append(x4)
        data['bits'].append(bits)
        data['cam'].append(val['cam_name'])
        data['frame'].append(val['framenumber'])

df = pd.DataFrame(data)
for bits, gdf in df.groupby('bits'):
    print(gdf.describe())
    print(len(gdf))
