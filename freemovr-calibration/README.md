## Testing without OpenCV

Run tests with:

    cargo test --release

## Testing with OpenCV

If you have OpenCV on your pkg-config path, run tests with:

    cargo test  --release --features "opencv"

If you have OpenCV installed to `~/devroot`, run tests with:

    PKG_CONFIG_PATH=$HOME/devroot/lib/pkgconfig cargo test --release --features "opencv"
