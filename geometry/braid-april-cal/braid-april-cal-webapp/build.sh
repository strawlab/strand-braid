#!/bin/bash
set -o errexit

# Build the project with Trunk. Installs into the `dist` directory.
# Install trunk as described here https://trunkrs.dev/#install
trunk build --release

# Run with Trunk development server using:
#     trunk serve
