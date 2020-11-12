from __future__ import print_function
import pandas as pd
import sys
import os
import subprocess
import numpy as np

metadata_fname = sys.argv[1]

metadata_df = pd.read_csv(metadata_fname, comment='#')#, skip_blank_lines=True)
print(metadata_df.head())

mydir = os.path.split(__file__)[0]
kalmanize_script = os.path.join(mydir,'kalmanize.py')
for fname in metadata_df['filename']:

    # skip blank lines in file
    try:
        if np.isnan(fname):
            continue
    except TypeError as err:
        pass

    kalm_fname = os.path.splitext(fname)[0] + '.kalmanized.csv'

    if not os.path.exists(kalm_fname):
        cmd = 'python %s %s' % (kalmanize_script, fname)
        print(cmd)
        try:
            subprocess.check_call(cmd,shell=True)
        except Exception as err:
            print('FAILED: %s' % err)
