#!/bin/bash
set -euo pipefail

# Build the project with Trunk. Installs into the `dist` directory.
# Install trunk as described here https://trunk-rs.github.io/trunk/guide/getting-started/installation.html
trunk build --release

# Run with Trunk development server using:
#    trunk serve
