# fmf-cli

## Introduction

The `fmf` command line program from
https://github.com/strawlab/strand-braid/tree/main/fmf/fmf-cli can be used for a
variety of tasks with `.fmf` files, especially converting to and from other
formats.

## Usage

Here is the output `fmf --help`:

```
work with .fmf (fly movie format) files

Usage: fmf <COMMAND>

Commands:
  export-fmf     export an fmf file
  info           print information about an fmf file
  export-jpeg    export a sequence of jpeg images
  export-png     export a sequence of png images
  export-y4m     export to y4m (YUV4MPEG2) format
  export-mp4     export to mp4
  import-images  import a sequence of images, converting it to an FMF file
  help           Print this message or the help of the given subcommand(s)

Options:
  -h, --help     Print help
  -V, --version  Print version
```

## Installation

This program is packaged with the Strand Camera and Braid for Ubuntu 20.04
starting with `strand-braid` version 0.11.0 available at the [`strand-braid`
releases page](https://github.com/strawlab/strand-braid/releases).

The program can be built from source when [the Rust toolchain is
installed](https://rustup.rs/) and by running the following command from the
command line:

    cargo build --release --no-default-features

To compile for Windows with support for the VP8 and VP9 codecs, inspect the
contents of the `windows-build.bat` file to figure out the required
dependencies, install them, and then run `windows-build.bat`.
