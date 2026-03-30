from __future__ import print_function
import pandas as pd
import sys
import matplotlib.pyplot as plt
import scipy.misc
import os
import numpy as np
import warnings
import read_detection_csv

metadata_fname = sys.argv[1]

metadata_df = pd.read_csv(metadata_fname, comment='#')

do_any_plot = False
plot_velocity = False

fig = plt.figure()
ax_tnt = fig.add_subplot(2,1,1)
ax_cntl = fig.add_subplot(2,1,2)

for (category, category_df) in metadata_df.groupby('category'):
    print("category: %s ------" % category)
    for (genotype, genotype_df) in category_df.groupby('genotype'):
        print("  genotype: %s ------" % genotype)
        for (food_name, food_df) in genotype_df.groupby('food_type'):
            print("    food_name: %s ------" % food_name)
            for (fname, fly_df) in food_df.groupby('filename'):
                print("      fname: %s" % fname)
                food_x = fly_df.iloc[0]['food_x']
                food_y = fly_df.iloc[0]['food_y']

                jpeg_fname = os.path.splitext(fname)[0] + '.jpg'
                jpeg = scipy.misc.imread(jpeg_fname)
                df = read_detection_csv.get_df(fname)

                food_distance = np.sqrt((df['x'] - food_x)**2 + (df['y'] - food_y)**2)
                food_dist_threshold = fly_df['food_radius'].iloc[0]

                first_valid_frame = fly_df['first_valid_frame'].iloc[0]
                print('first_valid_frame',first_valid_frame)

                on_food_condition = food_distance < food_dist_threshold
                on_food_idxs = np.nonzero(on_food_condition)[0]

                print(on_food_idxs.shape)
                if len(on_food_idxs) >= 1:
                    if not np.isnan(first_valid_frame):
                        # We had some bad tracking prior to this time, do not consider
                        # cases where detected fly to food ditance less than threshold prior
                        # to this time.
                        on_food_idxs = on_food_idxs[ on_food_idxs > first_valid_frame ]
                    first_food_idx = on_food_idxs[0]
                else:
                    first_food_idx = None

                if first_food_idx is not None:
                    last_idx = first_food_idx + 1000

                    x_vals = df['x'].iloc[first_food_idx:last_idx]
                    y_vals = df['y'].iloc[first_food_idx:last_idx]
                    if genotype == 'GMR60D05>TNTin':
                        ax_cntl.plot( x_vals, y_vals, '.', label=fname)
                        ax_cntl.set_title(genotype)
                        #ax_cntl.set_axes('equal')
                    elif genotype == 'GMR60D05>TNTe':
                        ax_tnt.plot( x_vals, y_vals, '.', label=fname)
                        ax_tnt.set_title(genotype)
                        #ax_tnt.set_axes('equal')
                    else:
                        print('not plotting genotype %s' % genotype)

                if not do_any_plot:
                    continue

                if 1:
                    plt.figure()
                    plt.imshow(jpeg, interpolation='nearest', cmap='jet')
                    plt.colorbar()
                    plt.xlabel('x (px)')
                    plt.ylabel('y (px)')
                    plt.plot(df['x'],df['y'],'.')

                if plot_velocity:
                    warnings.warn('pretending that we have regularly spaced data. FIXME. TODO!!!')
                    dx = np.gradient(df['x'].values)
                    dy = np.gradient(df['y'].values)
                    dt = np.gradient(df['timestamp'].values)
                    ddist = np.sqrt(dx**2 + dy**2)
                    hvel = ddist/dt

                fig = plt.figure()
                ax = fig.add_subplot(2,1,1)
                #ax.plot(df['timestamp'],food_distance,'.')
                ax.plot(food_distance,'.')
                ax.axhline(food_dist_threshold)
                if first_food_idx is not None:
                    #ax.axvline( df['timestamp'].iloc[first_food_idx] )
                    ax.axvline( first_food_idx )
                #plt.xlabel('time (sec)')
                plt.xlabel('time (frame)')
                plt.ylabel('distance from food (px)')
                ax.set_title(fname)

                if plot_velocity:

                    ax = fig.add_subplot(2,1,2, sharex=ax)
                    #ax.plot(df['timestamp'],hvel,'.')
                    ax.plot(hvel,'.')
                    ax.axhline(food_dist_threshold)
                    if first_food_idx is not None:
                        #ax.axvline( df['timestamp'].iloc[first_food_idx] )
                        ax.axvline( first_food_idx )
                    #plt.xlabel('time (sec)')
                    plt.xlabel('time (frame)')
                    plt.ylabel('speed (px/sec)')
                    ax.set_title(fname)

                plt.show()
#ax_cntl.legend()
#ax_tnt.legend()

plt.show()
