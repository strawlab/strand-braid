#!/bin/bash -x
set -euo pipefail

# Build the project with Trunk. Installs into the `dist` directory.
trunk build --features ads-webasm/obj

# Run with Trunk development server using:
#    trunk serve --features ads-webasm/obj
