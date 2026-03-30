import pandas as pd
import sys
import matplotlib.pyplot as plt
import numpy as np
import os
import scipy.misc
import read_detection_csv

fname = sys.argv[1]

df = pd.read_csv(fname,comment='#')
base1 = os.path.splitext(fname)[0]
base2 = os.path.splitext(base1)[0]
raw_df = read_detection_csv.get_df(base2+'.csv')

if 1:
    jpeg_fname = base2 + '.jpg'
    jpeg = scipy.misc.imread(jpeg_fname) # install python pillow package if AttributeError

    plt.figure()
    plt.imshow(jpeg, interpolation='nearest', cmap='gray',zorder=-100)
    plt.colorbar()
    plt.xlabel('x (px)')
    plt.ylabel('y (px)')

    for (obj_id, fly_df) in df.groupby('obj_id'):
        plt.plot(fly_df['pos_x_pix'],fly_df['pos_y_pix'],lw=2,label='id %d'%obj_id)

    plt.plot(raw_df['x_px'],raw_df['y_px'],'k.',zorder=-90)
    plt.title(fname)
    plt.legend()

if 1:
    fig = plt.figure()
    x_ax = fig.add_subplot(4,1,1)
    x_ax.set_title(fname)
    y_ax = fig.add_subplot(4,1,2,sharex=x_ax)
    vel_ax = fig.add_subplot(4,1,3,sharex=x_ax)
    error_ax = fig.add_subplot(4,1,4,sharex=x_ax)
    x_ax.plot(raw_df['frame'],raw_df['x'],'k.',label='observations',zorder=-99)
    y_ax.plot(raw_df['frame'],raw_df['y'],'k.',label='observations',zorder=-99)
    for (obj_id, fly_df) in df.groupby('obj_id'):
        x_ax.plot(fly_df['frame'],fly_df['pos_x'],label='id %d'%obj_id)

        y_ax.plot(fly_df['frame'],fly_df['pos_y'],label='id %d'%obj_id)

        speed = np.sqrt((fly_df['vel_x'].values**2 + fly_df['vel_y'].values**2))
        vel_ax.plot(fly_df['frame'],speed,label='fly %d'%obj_id)

        error = np.sqrt( fly_df['P00'].values**2 + fly_df['P11'].values**2 )
        error_ax.plot(fly_df['frame'],error,label='fly %d'%obj_id)

    x_ax.set_ylabel('x (meters)')
    y_ax.set_ylabel('y (meters)')
    vel_ax.set_ylabel('speed (meters/sec)')
    error_ax.set_ylabel('squared error (meters**2)')
    error_ax.set_xlabel('time (frames)')
    plt.legend()

if 1:
    fig = plt.figure()
    x_ax = fig.add_subplot(2,1,1)
    x_ax.set_title(fname)
    y_ax = fig.add_subplot(2,1,2,sharex=x_ax)
    x_ax.plot(raw_df['frame'],raw_df['x_px'],'k.',label='observations',zorder=-99)
    y_ax.plot(raw_df['frame'],raw_df['y_px'],'k.',label='observations',zorder=-99)
    for (obj_id, fly_df) in df.groupby('obj_id'):
        x_ax.plot(fly_df['frame'],fly_df['pos_x_pix'],label='id %d'%obj_id)
        y_ax.plot(fly_df['frame'],fly_df['pos_y_pix'],label='id %d'%obj_id)

    x_ax.set_ylabel('x (pixels)')
    y_ax.set_ylabel('y (pixels)')
    plt.legend()

plt.show()
