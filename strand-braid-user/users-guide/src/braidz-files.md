# `.braidz` files

A `.braidz` file contains the results of realtime tracking, the tracking
parameters, and so on.

## Online viewer

An online viewer for `.braidz` files is at [braidz.strawlab.org](https://braidz.strawlab.org/).

## Analysis scripts

The following plots were made with the file [20201112_133722.braidz](http://strawlab-cdn.com/assets/20201112_133722.braidz).

### `braid-analysis-plot-data2d-timeseries.py`

![braid-analysis-plot-data2d-timeseries.png](braid-analysis-plot-data2d-timeseries.png)

### `braid-analysis-plot-kalman-estimates-timeseries.py`

![braid-analysis-plot-kalman-estimates-timeseries.png](braid-analysis-plot-kalman-estimates-timeseries.png)

### `braid-analysis-plot3d.py`

![braid-analysis-plot3d.png](braid-analysis-plot3d.png)

## File Format

A `.braidz` file is actually a ZIP file with specific contents. It can be
helpful to know about these specifics when problems arise.

### Showing the contents of a `.braidz` file

You can show the filenames inside a .braidz file with
`unzip -l FILENAME.braidz`.

### Extracting a `.braidz` file

You can extract a `.braidz` file to its contents with `unzip FILENAME.braidz`.

### Creating a `.braidz` file

You can create a new `.braidz` file with:

    cd BRAID_DIR
    zip ../FILENAME.braidz *

Note, your `.braidz` file should look like this - with no directories other than
`images/`.

```
$ unzip -l 20191125_093257.braidz
Archive:  20191125_093257.braidz
zip-rs
  Length      Date    Time    Name
---------  ---------- -----   ----
       97  2019-11-25 09:33   README.md
      155  2019-11-25 09:33   braid_metadata.yml
        0  2019-11-25 09:33   images/
   308114  2019-11-25 09:33   images/Basler_22005677.png
   233516  2019-11-25 09:33   images/Basler_22139107.png
   283260  2019-11-25 09:33   images/Basler_22139109.png
   338040  2019-11-25 09:33   images/Basler_22139110.png
       78  2019-11-25 09:33   cam_info.csv.gz
     2469  2019-11-25 09:33   calibration.xml
      397  2019-11-25 09:33   textlog.csv.gz
   108136  2019-11-25 09:33   kalman_estimates.csv.gz
      192  2019-11-25 09:33   trigger_clock_info.csv.gz
       30  2019-11-25 09:33   experiment_info.csv.gz
     2966  2019-11-25 09:33   data_association.csv.gz
   138783  2019-11-25 09:33   data2d_distorted.csv.gz
---------                     -------
  1416233                     15 files
```

Note that the following is NOT a valid `.braidz` file because it has a leading
directory name for each entry.

```
$ unzip -l 20191119_114103.NOT-A-VALID-BRAIDZ
Archive:  20191119_114103.NOT-A-VALID-BRAIDZ
  Length      Date    Time    Name
---------  ---------- -----   ----
        0  2019-11-19 11:41   20191119_114103.braid/
       97  2019-11-19 11:41   20191119_114103.braid/README.md
      155  2019-11-19 11:41   20191119_114103.braid/braid_metadata.yml
        0  2019-11-19 11:41   20191119_114103.braid/images/
   320906  2019-11-19 11:41   20191119_114103.braid/images/Basler_22005677.png
   268847  2019-11-19 11:41   20191119_114103.braid/images/Basler_22139107.png
   308281  2019-11-19 11:41   20191119_114103.braid/images/Basler_22139109.png
   346232  2019-11-19 11:41   20191119_114103.braid/images/Basler_22139110.png
   225153  2019-11-19 11:41   20191119_114103.braid/images/Basler_40019416.png
       86  2019-11-19 11:41   20191119_114103.braid/cam_info.csv.gz
     2469  2019-11-19 11:41   20191119_114103.braid/calibration.xml
       10  2019-11-19 11:41   20191119_114103.braid/textlog.csv.gz
       10  2019-11-19 11:41   20191119_114103.braid/kalman_estimates.csv.gz
       10  2019-11-19 11:41   20191119_114103.braid/trigger_clock_info.csv.gz
       10  2019-11-19 11:41   20191119_114103.braid/experiment_info.csv.gz
       10  2019-11-19 11:41   20191119_114103.braid/data_association.csv.gz
 20961850  2019-11-19 12:17   20191119_114103.braid/data2d_distorted.csv.gz
---------                     -------
 22434126                     17 files
```
