import seaborn as sns
import pandas as pd
import sys
import matplotlib.pyplot as plt

summary_fname = sys.argv[1]
summary_df = pd.read_csv(summary_fname, comment='#')

def make_fig(ys, summary_df):
    fig,axes = plt.subplots(nrows=len(ys),ncols=1)

    for i, this_y in enumerate(ys):
        ax = axes[i]
        sns.boxplot(x="genotype", y=this_y, data=summary_df, ax=ax)
        #sns.swarmplot(x="genotype", y=this_y, data=summary_df, ax=ax, color='k')

make_fig(['max_food_dist','mean_food_dist','median_food_dist','frame_count'], summary_df)
make_fig(['frames_to_reach_1cm','frames_to_reach_2cm','frames_to_reach_5cm','frames_to_reach_10cm'], summary_df)

plt.show()
