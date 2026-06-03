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

## Network communication overview

The connection between Braid and remote Strand Camera instances is bidirectional:

1. **Strand Camera → Braid (TCP)**: Strand Camera connects to the Braid HTTP
   server (specified by `--braid-url`) to register itself and send updates.
2. **Braid → Strand Camera (TCP)**: Braid connects back to each Strand Camera's
   HTTP server to send commands (e.g. frame offsets, recording start/stop).
3. **Strand Camera → Braid (UDP)**: Strand Camera sends low-latency 2D feature
   detection data to the Braid UDP port.

For remote cameras (Strand Camera on a different machine than Braid), all three
paths must be reachable across the network.

When Strand Camera starts, it automatically determines which local network
interface IP to advertise to Braid for the return TCP connection (path 2 above),
based on the network interface that would be used to reach the Braid server IP.
This ensures Braid can connect back to Strand Camera even when they are on
different machines.

The low-latency UDP path is likewise specified by Braid and returned to Strand
Camera upon initial connection.

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

To start Strand Camera as a remote camera for Braid, run `strand-cam` with the
command line argument `--braid-url <URL>` specifying the URL for the braid HTTP
address. The camera name should also be specified on the command line using
`--camera-name <CAMERA NAME>`. Select the camera vendor backend with
`--camera-backend pylon` (the default) or `--camera-backend vimba`.

In the following example, the Strand Camera will open the camera named
`Camera-12345` and will connect to Braid running at `http://192.168.1.10:44444`.

```ignore
strand-cam --camera-backend pylon --camera-name Camera-12345 --braid-url http://192.168.1.10:44444/?token=<TOKEN>
```

The `?token=<TOKEN>` query parameter is required when the Braid server is
listening on a non-loopback address and strand camera has not made a connection
recently. (Strand Camera uses this for an initial connection and then stores a
cookie that lets it reconnect without the token in the future.) Braid prints the
full URL with the token on startup — use that URL directly.

## Overriding the Strand Camera HTTP server address

In rare cases (e.g. complex network configurations), Strand Camera's automatic
IP detection may choose the wrong network interface. You can override the
address that Strand Camera advertises to Braid by setting the `http_server_addr`
field in the relevant `[[cameras]]` entry in the Braid `.toml` file:

```toml
[[cameras]]
name = "Camera-1"
start_backend = "remote"
# Override the IP that braid uses to connect back to this strand-cam instance.
# Replace 192.168.1.20 with the IP of the remote camera computer.
http_server_addr = "192.168.1.20:0"
```

The port `0` instructs the operating system to assign a free port automatically.
You may also specify a fixed port number if needed (e.g. for firewall rules).
