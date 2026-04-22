# Building for Development

Note: our continuous integration system builds `strand-braid` daily using the
file `.gitlab-ci.yml`. Unfortunately due to various dependencies on
closed-source software, we cannot offer a "build from scratch" script that will
build everything automatically on your machine. Nevertheless, `.gitlab-ci.yml`
can serve as a reference for your builds.

## Dependencies which are not free, open-source software

| Dependency | Available | Description | Usage in `strand-braid` |
| :--- | :--- | :--- | :--- |
| [Intel IPP (Integrated Performance Primitives)](https://www.intel.com/content/www/us/en/developer/tools/oneapi/ipp.html) | see [here](https://www.intel.com/content/www/us/en/developer/tools/oneapi/ipp-download.html) | SIMD for high performance image processing on x86 | optional, used when the Cargo feature `use_ipp` is enabled. |
| [Basler Pylon](https://www.baslerweb.com/pylon) | free download, [not available for automatic use](https://github.com/basler/pypylon/issues/521#issuecomment-1206256554) | drivers for Basler cameras | used in `strand-cam-pylon` |
| [Allied Vision Technologies Vimba X](https://www.alliedvision.com/en/support/software-downloads/vimba-x-sdk/vimba-x) | free download, not available for automatic use | drivers for Allied Vision Technologies cameras | used in `strand-cam-vimba` |

## Prerequisites

[Install rust](https://rustup.rs/)

[Install trunk](https://trunk-rs.github.io/trunk/guide/getting-started/installation.html)

Install your camera drivers. Currently Basler Pylon and Allied Vision Vimba are
supported.

First checkout the git repository into a location which will below be called
`/path/to/strand-braid`:

```
cd /path/to # <---- change this to a suitable filesystem directory
git clone https://github.com/strawlab/strand-braid
cd strand-braid # now in /path/to/strand-braid
```

## Strand Camera

First, build the browser user interface (BUI) for Strand Camera. This will build
files in `strand-cam/yew_frontend/dist` which get included in the Strand Cam
executable:

```
cd /path/to/strand-braid/strand-cam/yew_frontend
trunk build
```

Then, build the Strand Cam executable for Basler cameras using the Pylon
drivers, which must be preinstalled:

```
cd /path/to/strand-braid/strand-cam/strand-cam-pylon
cargo build --release
# By default, the executable will be put in /path/to/strand-braid/target/release/strand-cam-pylon
```

Alternatively or additionally, build the Strand Cam executable for Allied Vision
cameras using the Vimba drivers, which must be preinstalled:

```
cd /path/to/strand-braid/strand-cam/strand-cam-vimba
cargo build --release
# By default, the executable will be put in /path/to/strand-braid/target/release/strand-cam-vimba
```

Many compile-time options exist to adjust the exact features used, but the
instructions above should build a working copy of Strand Camera albeit with
potentially reduced features and performance.

## Braid

We will build `braid-run` which is the main runtime application we call "Braid".

First, build the browser user interface (BUI) for Braid. This will build files
in `braid/braid-run/braid_frontend/dist` which get included in the `braid-run`
executable:

```
cd /path/to/strand-braid/braid/braid-run/braid_frontend
./build.sh
```

Then, build the `braid-run` executable:

```
cd /path/to/strand-braid/braid/braid-run
cargo build --release
# By default, the executable will be put in /path/to/strand-braid/target/release/braid-run
```
