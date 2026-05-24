#!/bin/bash
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"

# Generate the third-party license bundle for the viewer's transitive
# dependencies. Trunk copies it into dist/ via the `copy-file` link in
# index.html. Install `cargo-bundle-licenses` first:
#   cargo install cargo-bundle-licenses --locked
(
    cd "$HERE"
    cargo bundle-licenses \
        --format yaml \
        --prefer MIT \
        --output "$HERE/THIRD-PARTY-LICENSES.yml"
)

# Build the project with Trunk. Installs into the `dist` directory.
# Install trunk as described here https://trunk-rs.github.io/trunk/guide/getting-started/installation.html
trunk build --release

# Run with Trunk development server using:
#    trunk serve
