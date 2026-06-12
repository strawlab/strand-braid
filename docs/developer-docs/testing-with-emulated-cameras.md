# Testing with emulated cameras

The Basler Pylon driver can emulate cameras, which allows running real
`strand-cam` and `braid-run` binaries — including image acquisition, feature
detection, and the full HTTP control API — without any camera hardware. This
is useful for smoke testing and for developing against the browser UI or the
scripting API.

## Prerequisites

- Built `strand-cam` and `braid-run` binaries (see
  [building-for-development.md](building-for-development.md)).
- The Basler Pylon SDK installed (the version used by CI is installed by
  `_packaging/install-pylon-linux.sh`).
- The `libpylon-cabi` shim library, which `strand-cam` loads at runtime.
  Precompiled shims are available from
  <https://strawlab.org/assets/libpylon-cabi/precompiled/> (this is also where
  the .deb packaging CI gets it; see
  `.github/actions/package-strand-braid-deb/action.yml`). If the shim is not
  installed in a standard library location, point the `PYLON_CABI` environment
  variable at the `.so` file.

## Emulated cameras

Setting the environment variable `PYLON_CAMEMU=N` makes the Pylon driver
enumerate `N` emulated cameras in addition to any real cameras. The emulated
cameras get the serial numbers `0815-0000`, `0815-0001`, … and therefore
appear in strand-braid under the camera names `Basler-0815-0000`,
`Basler-0815-0001`, and so on. Because each emulated camera has a distinct
serial number, multiple emulated cameras work without further configuration.

**Warning:** on a machine with real cameras attached, the emulated cameras
are enumerated *alongside* the real ones, and `strand-cam` without
`--camera-name` opens the first camera it finds — which may be real hardware.
Always pass `--camera-name` when testing on such machines.

Run a standalone Strand Camera on an emulated camera:

```sh
PYLON_CAMEMU=1 strand-cam \
    --camera-backend pylon \
    --camera-name Basler-0815-0000 \
    --no-browser \
    --http-server-addr 127.0.0.1:3440
```

Run Braid with three emulated cameras using the example configuration
`braid/simple.toml` (which names `Basler-0815-0000` through `-0002`; Braid
automatically spawns one `strand-cam` process per camera, looking for the
`strand-cam` executable next to the `braid-run` executable):

```sh
PYLON_CAMEMU=3 braid-run braid/simple.toml
```

## Smoke test

`smoke-tests/braid-camemu.sh` runs an end-to-end smoke test using the above:

1. It starts a standalone `strand-cam` with one emulated camera and exercises
   the HTTP control API using the `reset-background.py` demo script from
   `docs/user-docs/scripts/`, verifying in the log that the commands reached
   the feature detector.
2. It then starts `braid-run` with two emulated cameras, waits for them to
   synchronize, and runs `reset-background-braid-all-cams.py`, which
   discovers both cameras from Braid and commands each one, again verifying
   the effect in the logs.

```sh
# After building target/release/strand-cam and target/release/braid-run:
smoke-tests/braid-camemu.sh

# Or, with explicit binary and shim locations:
STRAND_BRAID_TARGET_DIR=target/release \
    PYLON_CABI=/path/to/libpylon-cabi.so \
    smoke-tests/braid-camemu.sh
```

The script exits 0 and prints `PASSED` on success. It requires `python3` with
the `requests` library and `curl`. Ports can be overridden with
`STRAND_CAM_PORT` and `BRAID_PORT` if the defaults (3477 and 44477) collide
with something on your machine.

## Continuous integration

The GitLab CI pipeline runs this smoke test in the `smoke_test_camemu` job
(see `.gitlab-ci.yml`), using the binaries built by the `strand-cam-binary`
and `braid-run-binary` artifact jobs, the Pylon SDK installed by
`_packaging/install-pylon-linux.sh`, and the precompiled shim downloaded from
strawlab.org. The job does not exist on GitHub Actions because the Pylon SDK
is downloaded from an internal server which is not reachable from there.
