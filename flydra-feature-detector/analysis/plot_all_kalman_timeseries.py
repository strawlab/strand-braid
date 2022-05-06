from __future__ import print_function
import pandas as pd
import sys
import matplotlib.pyplot as plt
import seaborn as sns
import pandas as pd
import scipy.misc
import os
import numpy as np
import warnings
import read_detection_csv
import collections
import yaml

def get_dt(kalman_filename):
    fd = open(kalman_filename,mode='r')
    buf = fd.read(100)
    if buf.startswith('#'):
        line0 = buf.split('\n')[0]
        assert line0.startswith('# ')
        yaml_buf = line0[2:]
        yaml_data = yaml.safe_load(yaml_buf)
        dt = yaml_data['dt']
    else:
        base1 = os.path.splitext(kalman_filename)[0]
        base2 = os.path.splitext(base1)[0]
        csv_filename = base2 + '.csv'
        df = read_detection_csv.get_df(csv_filename)
        dt = read_detection_csv.calculate_dt_1(df['timestamp'].values)
    return dt

def get_frames_to_reach(thresh, left_food_df):
    df = left_food_df[ left_food_df['food_distance'] >=thresh ]
    if len(df) == 0:
        return np.nan
    fthresh = df['frame'].iloc[0]
    f0 = left_food_df['frame'].iloc[0]
    n_frames = fthresh - f0
    return n_frames

metadata_fname = sys.argv[1]

data_base = os.path.split(metadata_fname)[0]
metadata_df = pd.read_csv(metadata_fname, comment='#')

skip_indiv_plots = False
plot_velocity = True

topview_genotypes = ['GMR60D05>TNTe','GMR60D05>TNTin','wildtype']
topview_fig, topview_axes = plt.subplots(nrows=len(topview_genotypes),ncols=1,figsize=(10,15),sharex=True,sharey=True)

# ax_tnt = topview_fig.add_subplot(3,1,1)
# ax_cntl = topview_fig.add_subplot(3,1,2,sharex=ax_tnt,sharey=ax_tnt)
# ax_wt = topview_fig.add_subplot(3,1,3,sharex=ax_tnt,sharey=ax_tnt)

count = 0

# only show yeast category A data
metadata_df = metadata_df[ metadata_df['category']=='A']
metadata_df = metadata_df[ metadata_df['food_type']=='yeast']

summary_data = collections.defaultdict(list)
food_distance_by_genotype = collections.defaultdict(list)
genotype_n_count = collections.defaultdict(int)

for (category, category_df) in metadata_df.groupby('category'):
    print("category: %s ------" % category)
    for (genotype, genotype_df) in category_df.groupby('genotype'):
        print("  genotype: %s ------" % genotype)
        for (food_type, food_df) in genotype_df.groupby('food_type'):
            print("    food_type: %s ------" % food_type)
            for (fname, fly_df) in food_df.groupby('filename'):
                print("      fname: %s" % fname)
                food_x = fly_df.iloc[0]['food_x']
                food_y = fly_df.iloc[0]['food_y']

                base1 = os.path.splitext(fname)[0]
                jpeg_fname = os.path.join(data_base,base1 + '.jpg')
                jpeg = scipy.misc.imread(jpeg_fname)
                # df = read_detection_csv.get_df(fname)
                kalman_filename = os.path.join(data_base,base1+'.kalmanized.csv')
                kalman_df = pd.read_csv(kalman_filename, comment='#')
                dt = get_dt(kalman_filename)

                csv_filename = os.path.join(data_base,base1+'.csv')

                A, Ainv = read_detection_csv.get_A_Ainv(csv_filename)
                diam_meters = 0.197
                diam_pixels = 1000.0
                scale = diam_meters/diam_pixels

                food_xy_pixels_homog = np.array([[food_x],[food_y],[1.0]])
                food_xy_meters_homog = np.dot( A, food_xy_pixels_homog)
                food_xy_meters = food_xy_meters_homog[:2] / food_xy_meters_homog[2]
                food_distance = np.sqrt((kalman_df['pos_x'] - food_xy_meters[0])**2 + (kalman_df['pos_y'] - food_xy_meters[1])**2)
                food_dist_threshold = fly_df['food_radius'].iloc[0] * scale
                leave_dist_threshold = food_dist_threshold + 0.005

                first_valid_frame = fly_df['first_valid_frame'].iloc[0]
                #print('first_valid_frame',first_valid_frame)

                on_food_condition = food_distance < food_dist_threshold
                on_food_idxs = np.nonzero(on_food_condition)[0]
                off_food_idxs = np.nonzero(food_distance > leave_dist_threshold)[0]

                if len(on_food_idxs) >= 1:
                    if not np.isnan(first_valid_frame):
                        # We had some bad tracking prior to this time, do not consider
                        # cases where detected fly to food distance less than threshold prior
                        # to this time.
                        on_food_idxs = on_food_idxs[ on_food_idxs > first_valid_frame ]
                        off_food_idxs = off_food_idxs[ off_food_idxs > first_valid_frame ]
                    first_food_idx = on_food_idxs[0]
                    first_off_food_idx = off_food_idxs[ off_food_idxs>first_food_idx ][0]
                else:
                    first_food_idx = None
                    first_off_food_idx = None

                # start_idx = first_food_idx
                start_idx = first_off_food_idx
                last_idx = None

                left_food_df = None
                start_frame = None
                last_frame = None

                if start_idx is not None:
                    duration_sec = 30.0
                    n_frames = duration_sec / dt

                    start_frame = kalman_df['frame'].iloc[start_idx]
                    last_frame = start_frame + n_frames
                    left_food_cond = (start_frame <= kalman_df['frame'].values) & (kalman_df['frame'].values <= last_frame)
                    left_food_df = kalman_df.iloc[left_food_cond].copy()

                    x_vals = left_food_df['pos_x']
                    y_vals = left_food_df['pos_y']

                    food_distance = np.sqrt((x_vals - food_xy_meters[0])**2 + (y_vals - food_xy_meters[1])**2)
                    left_food_df['food_distance'] = food_distance
                    max_food_dist = np.max(food_distance)
                    median_food_dist = np.median(food_distance)
                    mean_food_dist = np.mean(food_distance)

                    food_distance_by_genotype[genotype].extend(list(food_distance))

                    summary_data['genotype'].append(genotype)
                    summary_data['filename'].append(fname)
                    summary_data['max_food_dist'].append(max_food_dist)
                    summary_data['median_food_dist'].append(median_food_dist)
                    summary_data['mean_food_dist'].append(mean_food_dist)
                    summary_data['frame_count'].append(len(y_vals))
                    for cm_dist in [1,2,5,10]:
                        summary_data['frames_to_reach_%dcm'%cm_dist].append(get_frames_to_reach(cm_dist/100.0, left_food_df))

                    if genotype in topview_genotypes:
                        ax_idx = topview_genotypes.index(genotype)
                        ax = topview_axes[ax_idx]
                        ax.plot( x_vals, y_vals, '.', ms=0.5, label=fname)
                        genotype_n_count[genotype] += 1
                else:
                    print('******************* no on-food bout detected: %s' % fname)

                if skip_indiv_plots:
                    continue

                if count > 5:
                    continue
                count += 1

                # ----------------------------------------

                if 1:

                    raw_df = read_detection_csv.get_df(csv_filename)

                    plt.figure()
                    plt.imshow(jpeg, interpolation='nearest', cmap='gray', zorder=-100)
                    plt.colorbar()
                    for obj_id, obj_df in kalman_df.groupby('obj_id'):
                        plt.plot(obj_df['pos_x_pix'],obj_df['pos_y_pix'],'-')
                    plt.plot(raw_df['x_px'],raw_df['y_px'],'k.',ms=5,zorder=-99)
                    plt.xlabel('x (px)')
                    plt.ylabel('y (px)')
                    plt.title(fname)

                fig = plt.figure()
                ax = fig.add_subplot(2,1,1)
                for obj_id, obj_df in kalman_df.groupby('obj_id'):
                    this_food_distance = np.sqrt((obj_df['pos_x'] - food_xy_meters[0])**2 + (obj_df['pos_y'] - food_xy_meters[1])**2)
                    ax.plot(obj_df['frame'],this_food_distance,'-')

                if left_food_df is not None:
                    used_frames = left_food_df['frame'].values
                    my_zeros = np.zeros_like(used_frames)
                    ax.plot( used_frames, my_zeros, 'kx' )

                ax.axhline(food_dist_threshold)
                if first_food_idx is not None:
                    ax.axvline( start_frame, lw=2 )
                    ax.axvline( last_frame, lw=2 )
                    ax.axvline( kalman_df['frame'].iloc[first_food_idx], lw=0.5 )
                    ax.axvline( kalman_df['frame'].iloc[first_off_food_idx], lw=0.5 )

                ax.set_xlabel('time (frame)')
                ax.set_ylabel('distance from food (m)')
                ax.set_title(fname)

                if plot_velocity:

                    ax = fig.add_subplot(2,1,2, sharex=ax)
                    for obj_id, obj_df in kalman_df.groupby('obj_id'):
                        this_speed = np.sqrt(obj_df['vel_x']**2+obj_df['vel_y']**2)
                        ax.plot(obj_df['frame'],this_speed,'-')
                    ax.set_ylim(0, 0.10)
                    ax.set_xlabel('time (frame)')
                    ax.set_ylabel('speed (m/sec)')
                    ax.set_title(fname)

summary_data = pd.DataFrame(summary_data)
summary_data.to_csv(os.path.join(data_base,'summary_data.csv'))

for genotype in topview_genotypes:
    ax_idx = topview_genotypes.index(genotype)
    ax = topview_axes[ax_idx]
    ax.set_title('%s (N=%d)'%(genotype,genotype_n_count[genotype]))

# topview_genotypes = ['GMR60D05>TNTe','GMR60D05>TNTin','wildtype']
hist_fig, hist_axes = plt.subplots(nrows=len(topview_genotypes),ncols=1,sharex=True,sharey=True)
for i,genotype in enumerate(topview_genotypes):
    ax = hist_axes[i]
    dist_data = np.array(food_distance_by_genotype[genotype])
    sns.distplot(dist_data,ax=ax,kde=False,axlabel='distance (cm)')
    ax.set_title('%s (n=%d, N=%d)'%(genotype,len(dist_data),genotype_n_count[genotype]))

topview_fig.savefig(os.path.join(data_base,'topviews.png'))
hist_fig.savefig(os.path.join(data_base,'distance_histograms.png'))
plt.show()
