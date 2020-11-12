# Testing web app

Here is what to do:

- Click "Select an OBJ file" and then select the `data/cylinder.obj` file.
- Enter a display size of 1024x768.
- In the "Input: Corresponding Points" section, click "Select a CSV file" and select `corresponding-points-advanced.csv`.
- In the "Output" section, click "Compute EXR" then click "Download EXR".

The exr file should look like:

![out-simple](out-simple.jpg)

In the "Advanced: Corresponding Points for Fine Tuning" section:

- Click "Compute Corresponding Points", the "Download Corresponding Points"
- Upload new corresponding points by clicking "Select a CSV file" and select `corresponding-points-simple.csv`.
- In the "Output" section, click "Compute EXR" then click "Download EXR".

The exr file should look like:

![out-advanced](out-advanced.jpg)
