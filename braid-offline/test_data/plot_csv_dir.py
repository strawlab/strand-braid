import sys
import os
import numpy as np
import pandas as pd
import matplotlib.pyplot as plt

dirname = sys.argv[1]
print dirname

data2d_df = pd.read_csv(os.path.join(dirname,'data2d_distorted.csv'))
kest_df = pd.read_csv(os.path.join(dirname,'kalman_estimates.csv'))

fig = plt.figure()
ax = fig.add_subplot(211)
for obj_id, df in kest_df.groupby('obj_id'):
    ax.plot( df['x']*1000.0, df['y']*1000.0, label='%d' % obj_id )
ax.legend()
ax.set_xlabel('x (mm)')
ax.set_ylabel('y (mm)')
ax.set_aspect('equal', adjustable='box')

ax = fig.add_subplot(212)
ax.plot( data2d_df['x'], data2d_df['y'], '.' )
ax.set_xlabel('x (px)')
ax.set_ylabel('y (px)')
ax.set_aspect('equal', adjustable='box')

fig,axes = plt.subplots(nrows=4,ncols=1,sharex=True)
axx, axy = axes[0], axes[1]
for obj_id, df in kest_df.groupby('obj_id'):
    axx.fill_between( df['frame'],
        (df['x'] + np.sqrt(df['P00']))*1000.0,
        (df['x'] - np.sqrt(df['P00']))*1000.0,
        alpha = 0.2, edgecolor='none')
    axx.plot( df['frame'], df['x']*1000.0 )
    axy.fill_between( df['frame'],
        (df['y'] + np.sqrt(df['P11']))*1000.0,
        (df['y'] - np.sqrt(df['P11']))*1000.0,
        alpha = 0.2, edgecolor='none')
    axy.plot( df['frame'], df['y']*1000.0 )

axx.set_ylabel('x (mm)')
axy.set_ylabel('y (mm)')

axu, axv = axes[2], axes[3]
axu.plot( data2d_df['frame'], data2d_df['x'], '.' )
axv.plot( data2d_df['frame'], data2d_df['y'], '.' )
axu.set_ylabel('x (px)')
axv.set_ylabel('y (px)')

plt.show()
