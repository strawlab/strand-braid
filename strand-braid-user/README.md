# nextgen-camera-users

Files and support for users of next generation camera software from the Straw
Lab.

## Downloads

The most recent files can be downloaded [here](https://github.com/strawlab/nextgen-camera-users/releases).

## Live 3D pose estimates

The script in this repository `scripts/flydra2_retransmit_udp.py` is an example
of parsing live, low-latency 3D tracking data.

## Analysis of saved trajectories from Braid

Remember, you can view your `.braidz` file at https://braidz.strawlab.org/.

## Questions, requests for help, etc.

 - [the online forum](https://groups.google.com/forum/#!forum/multicams)
 - [Github issues](https://github.com/strawlab/nextgen-camera-users/issues)

### Convert mainbrain `.h5` file to a simple `.h5` file.

For your own analysis, convert flydra mainbrain .h5 files to simple .h5
files with `flydra_analysis_export_flydra_hdf5`. These files are much simpler
and have only the 3D trajectories, so are much smaller and have already been
through smoothing. The format is documented
[here](https://strawlab.org/schemas/flydra/1.3).
