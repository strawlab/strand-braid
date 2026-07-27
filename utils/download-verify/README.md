# download-verify

Download a file from a URL and verify its SHA-256 hash, caching it on disk so
repeat calls just re-validate the existing file instead of re-downloading.

## Library

`download_verify::download_verify(url, dest, &Hash::Sha256(hex_digest))` is
used as a `[dev-dependencies]` in several crates' integration tests to fetch
known test fixtures (e.g. `.braidz` files) from `https://strawlab-cdn.com/assets/`.
See `src/lib.rs`'s own test and any `tests/*.rs` file elsewhere in the
workspace that imports this crate for examples.

## CLI

`src/bin/download-verify.rs` exposes the same function as a small CLI, for use
from shell scripts (e.g. `media-utils/tutorial-video-simulation/checkerboard-calibration/record.sh`):

```sh
cargo run -p download-verify --bin download-verify -- \
    --url https://strawlab-cdn.com/assets/some-file.mp4 \
    --sha256 <hex-encoded sha256> \
    --dest /path/to/save/some-file.mp4
```

Exits non-zero with an error message on a network failure or hash mismatch.
