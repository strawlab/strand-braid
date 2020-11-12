#!/bin/bash -x
set -o errexit

# rebuild with framework rpath
# This is not the right way to build a bundle. The bundle should include the framework with it.
rm -f ../target/debug/examples/one && cargo rustc --example one -- -C link-arg=-Wl,-rpath,/Library/Frameworks
