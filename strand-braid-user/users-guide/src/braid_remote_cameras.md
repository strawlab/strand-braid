# Remote Cameras for Braid

## What are "remote cameras"?

A remote camera, in the context of Braid, can be used to connect cameras on
separate computers over the network to an instance of Braid. One or more
instances of Strand Camera can thus run on computers other than the computer on
which Braid is running.

## Starting a remote camera

If a particular camera is marked by setting `start_backend = "remote"` in the
`[[cameras]]` section of the Braid configuration TOML file, `braid run` does not
attempt to start the camera but rather waits for a network connection from a
Braid-tuned variant of Strand Camera. Only once all cameras listed in the TOML
file have connected will Braid synchronize the cameras and allow recording of
data.

To start Strand Camera as a remote camera for Braid, run `strand-cam-pylon` (to
start Strand Camera) with the command line argument `--braid_addr <URL>`
specifying the URL for the braid HTTP address. The camera name should also be
specified on the command line, along with any other options.

In the following example, the Strand Camera will open the camera named
`Basler-12345` and will connect to Braid running at `http://127.0.0.1:44444`.

```ignore
strand-cam-pylon --camera-name Basler-12345 --braid_addr http://127.0.0.1:44444
```
