#!/bin/bash
set -o errexit

# Install trunk as described here https://trunk-rs.github.io/trunk/guide/getting-started/installation.html
trunk build --release
