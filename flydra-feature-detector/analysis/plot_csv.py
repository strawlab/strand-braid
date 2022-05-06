import pandas as pd
import sys
import matplotlib.pyplot as plt
import scipy.misc
import os
import read_detection_csv

fname = sys.argv[1]

jpeg_fname = os.path.splitext(fname)[0] + '.jpg'
jpeg = scipy.misc.imread(jpeg_fname) # install python pillow package if AttributeError
df = read_detection_csv.get_df(fname)
print(df.head())
t0 = df['timestamp'].iloc[0]

plt.figure()
plt.imshow(jpeg, interpolation='nearest', cmap='jet')
plt.colorbar()
plt.xlabel('x (px)')
plt.ylabel('y (px)')
plt.plot(df['x_px'],df['y_px'],'.')

if 1:
    # plot vs time (seconds)
    fig = plt.figure()
    ax = fig.add_subplot(2,1,1)
    ax.plot(df['timestamp']-t0,df['x_px'],'.')
    plt.xlabel('time (sec)')
    plt.ylabel('x (px)')

    ax = fig.add_subplot(2,1,2,sharex=ax)
    ax.plot(df['timestamp']-t0,df['y_px'],'.')
    plt.xlabel('time (sec)')
    plt.ylabel('y (px)')

if 1:
    # plot vs time (frames)
    fig = plt.figure()
    ax = fig.add_subplot(2,1,1)
    ax.plot(df['frame'],df['x_px'],'.')
    plt.xlabel('time (frame)')
    plt.ylabel('x (px)')

    ax = fig.add_subplot(2,1,2,sharex=ax)
    ax.plot(df['frame'],df['y_px'],'.')
    plt.xlabel('time (frame)')
    plt.ylabel('y (px)')

plt.show()
