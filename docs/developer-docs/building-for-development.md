# Building for Development

Note: our continuous integration system builds `strand-braid` daily using the
file `.gitlab-ci.yml`. Unfortunately due to various dependencies on
closed-source software, we cannot offer a "build from scratch" script that will
build everything automatically on your machine. Nevertheless, `.gitlab-ci.yml`
can serve as a reference for your builds.

## Dependencies which are not free, open-source software

| Dependency | Available | Description | Usage in `strand-braid` |
| :--- | :--- | :--- | :--- |
| [Basler Pylon](https://www.baslerweb.com/pylon) | free download, [not available for automatic use](https://github.com/basler/pypylon/issues/521#issuecomment-1206256554) | drivers for Basler cameras | loaded at runtime by `strand-cam --camera-backend pylon` |
| [Allied Vision Technologies Vimba X](https://www.alliedvision.com/en/support/software-downloads/vimba-x-sdk/vimba-x) | free download, not available for automatic use | drivers for Allied Vision Technologies cameras | loaded at runtime by `strand-cam --camera-backend vimba` |

## Prerequisites

[Install rust](https://rustup.rs/)

[Install trunk](https://trunk-rs.github.io/trunk/guide/getting-started/installation.html)
and the WASM compilation target (`rustup target add wasm32-unknown-unknown`).[^trunk]

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

Build the Strand Cam executable. A single `strand-cam` executable supports
both the Basler Pylon and Allied Vision Vimba backends. The vendor drivers are
loaded dynamically at runtime, so they do not need to be installed to build
(but the relevant driver must be installed to actually open a camera):

```
cd /path/to/strand-braid/strand-cam
cargo build --release --bin strand-cam
# By default, the executable will be put in /path/to/strand-braid/target/release/strand-cam
```

Select the camera vendor backend at runtime with `--camera-backend pylon` (the
default) or `--camera-backend vimba`.

Many compile-time options exist to adjust the exact features used, but the
instructions above should build a working copy of Strand Camera albeit with
potentially reduced features and performance.

## Braid

We will build `braid-run` which is the main runtime application we call "Braid":

```
cd /path/to/strand-braid/braid/braid-run
cargo build --release
# By default, the executable will be put in /path/to/strand-braid/target/release/braid-run
```

[^trunk]: The browser user interfaces (in `strand-cam/yew_frontend` and
    `braid/braid-run/braid_frontend`) are compiled by `trunk` and embedded
    into the executables automatically by `build.rs`.

## Testing without camera hardware

Once `strand-cam` and `braid-run` are built, they can be run and smoke tested
without any camera hardware using emulated Basler Pylon cameras. See
[testing-with-emulated-cameras.md](testing-with-emulated-cameras.md).
