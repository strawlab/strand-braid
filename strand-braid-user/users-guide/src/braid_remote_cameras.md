# Remote Cameras for Braid

## What are "remote cameras"?

A remote camera, in the context of Braid, can be used to connect cameras on
separate computers over the network to an instance of Braid. One or more
instances of Strand Camera can thus run on computers other than the computer on
which Braid is running.

## Starting a remote camera

If a particular camera is marked by setting `remote_camera = true` in the
`[[cameras]]` section of the Braid configuration TOML file, `braid run` does not
attempt to start the camera but rather waits for a network connection from a
Braid-tuned variant of Strand Camera. Only once all cameras listed in the TOML
file have connected will Braid synchronize the cameras and allow recording of
data.

To start Strand Camera as a remote camera for Braid, run
`braid-strand-cam-pylon` (to start a Braid-specific version of Strand Camera)
with the command line argument specifying the URL for the braid HTTP address.
The camera should also be specified on the command line, along with any other
options.

In the following example, the Strand Camera will open the camera named
`Basler-12345` and will connect to Braid running at `http://127.0.0.1:44444`.
Strand Camera will itself open the user interface at `http://127.0.0.1:12345/`.

    braid-strand-cam-pylon --camera-name Basler-12345 --http-server-addr 127.0.0.1:12345 http://127.0.0.1:44444
