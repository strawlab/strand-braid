import pandas as pd
import numpy as np

def calculate_dt_1(times):
    '''based on lowest intervals between timestamps, calculate inter-frame-interval'''
    dts = times[1:]-times[:-1]
    order_idx = dts.argsort()
    first_dts = []
    for idx in order_idx:
        this_dt = dts[idx]
        if this_dt == 0.0:
            continue
        first_dts.append(this_dt)
        if len(first_dts) >= 1000:
            break
    if len(first_dts) < 1:
        raise RuntimeError('cannot calculate dt')
    dt = np.median(first_dts)
    return dt

def calculate_frame(times, max_allowable_error_seconds=0.020):
    '''infer a reasonable value for an integer frame number based on timestamp'''
    dt = calculate_dt_1(times)
    t0 = times[0]
    times_relative = times-t0
    frame = times_relative/dt
    frame = np.round(frame).astype(np.uint32)
    time_predicted = frame * dt + t0
    time_error = abs(times - time_predicted)
    if np.max(time_error) > max_allowable_error_seconds:
        raise RuntimeError('error larger than %s msec calculating frames'%(max_allowable_error_seconds*1000.0))
    return frame

def migrate_df_1(old_df):
    '''migrate version of DataFrame'''
    times = old_df['timestamp'].values

    frame = calculate_frame(times)
    df = old_df.assign(frame=frame)
    return df

def get_A_Ainv(filename):
    diam_meters = 0.197
    diam_pixels = 1000.0
    scale = diam_meters/diam_pixels
    A = np.array( [[scale, 0, 0],
        [0, scale, 0],
        [0, 0, 1]])
    Ainv = np.array( [[1./scale, 0, 0],
        [0, 1./scale, 0],
        [0, 0, 1]])
    return A, Ainv

def convert_to_meters(filename,orig_df):
    A, Ainv = get_A_Ainv(filename)
    df = orig_df.copy()
    xy_homogeneous = np.array([df['x'].values, df['y'].values, np.ones(len(df))])
    xy_meters_homogeneous = np.dot(A,xy_homogeneous)
    xy_meters = xy_meters_homogeneous[:2] / xy_meters_homogeneous[2]
    del df['x']
    del df['y']
    df['x'] = xy_meters[0]
    df['y'] = xy_meters[1]
    df['x_px'] = orig_df['x']
    df['y_px'] = orig_df['y']
    return df

def get_df(filename):
    df = pd.read_csv(filename,comment='#')
    if 'frame' not in df.columns:
        df = migrate_df_1(df)
    return convert_to_meters(filename,df)
