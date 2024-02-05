# Remote Cameras for Braid

## What are "remote cameras"?

A remote camera, in the context of Braid, can be used to connect cameras on
separate computers over the network to an instance of Braid. One or more
instances of Strand Camera can thus run on computers other than the computer on
which Braid is running. Cameras can be specified in the Braid configuration
`.toml` file as being remote and remote cameras can be mixed with non-remote
cameras.

It can also be useful to launch cameras as "remote cameras" even if they run on
the same computer. For example, this can help distinguishing the source of
messages printed to the terminal.

## Relevant aspects of Braid configuration

(Relevant background reading: [Braid TOML configuration
files](braid_configuration_and_launching.md#braid-toml-configuration-files).)

If a particular camera is marked by setting `start_backend = "remote"` in the
`[[cameras]]` section of the Braid configuration TOML file, `braid run` does not
attempt to start the camera but rather waits for a network connection from
Strand Camera. Ensure the `start_backend` field of each relevant camera (in
`[[cameras]]`) is set to `"remote"`.

Only once all cameras listed in the TOML file have connected will Braid
synchronize the cameras and allow recording of data.

You may also want to specifically assign the IP and port of the mainbrain HTTP
server. See the reference documentation for [the `http_api_server_addr`
field](https://strawlab.org/strand-braid-api-docs/latest/braid_config_data/struct.MainbrainConfig.html#structfield.http_api_server_addr).

```toml
[mainbrain]
http_api_server_addr = "0.0.0.0:44444"

[[cameras]]
name = "Camera-1"
start_backend = "remote"

[[cameras]]
name = "Camera-2"
start_backend = "remote"
```

## Starting a remote camera

When launching Braid with a configuration file as above, the messages printed by
Braid will suggest the relevant arguments to use when starting Strand Camera as
a remote camera for Braid.

To start Strand Camera as a remote camera for Braid, run `strand-cam-pylon` (or
`strand-cam-vimba`) with the command line argument `--braid-url <URL>`
specifying the URL for the braid HTTP address. The camera name should also be
specified on the command line using `--camera-name <CAMERA NAME>`.

In the following example, the Strand Camera will open the camera named
`Camera-12345` and will connect to Braid running at `http://127.0.0.1:44444`.

```ignore
strand-cam-pylon --camera-name Camera-12345 --braid-url http://127.0.0.1:44444
```
