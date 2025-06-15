# show-timestamps

CLI program to display timestamps stored in video files saved by Strand Camera.

## Compilation and installation

The `show-timestamps` program is packaged and installed by the `strand-braid`
installer.

Alternatively, it can be installed using standard Rust tools. Here are
instructions about how to [install
Rust](https://www.rust-lang.org/tools/install). Once this is done, you can
install `show-timestamps` like this:


```bash
cd <path_to_strand_braid>/media-utils/show-timestamps
cargo install --path .
```


## Usage

Here is the output of `show-timestamps --help`:

```
Usage: show-timestamps [OPTIONS] <INPUTS>...

Arguments:
  <INPUTS>...
          Inputs. Either files (e.g. `file.mp4`) or TIFF image directories. The first TIFF file in a TIFF image directory is also accepted.

          For a TIFF image directory, images will be ordered alphabetically.

Options:
      --output <OUTPUT>
          Output format

          [default: summary]

          Possible values:
          - summary:     Print a summary in human-readable format
          - every-frame: Print a line for every frame in human-readable format
          - csv:         Print as comma-separated values with a row for every frame
          - srt:         Print as SubRip subtitle file (.srt)

      --timestamp-source <TIMESTAMP_SOURCE>
          Source of timestamp

          [default: best-guess]
          [possible values: best-guess, frame-info-recv-time, mp4-pts, misp-microsectime, srt-file]

  -p, --progress
          Show progress

  -h, --help
          Print help (see a summary with '-h')

  -V, --version
          Print version
```

## Example usage

To create a SubRip subtitle file which VLC will automatically open and use:

```bash
show-timestamps --output srt /some_path/movie.mp4  > /some_path/movie.srt
```

