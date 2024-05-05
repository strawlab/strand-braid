#!/bin/bash -x
set -o errexit

# Note: the software in this repository that requires opencv also work with
# opencv 4 as packaged by Ubuntu and Debian (and presumably other linuxes).
# Install this with `apt-get install libopencv-dev`. In our continuous
# integration system (for which this script is built), it is helpful to keep the
# setup time of the build environment to a minimum and thus we use our own
# packaging of opencv. This is substantially faster to install.

curl --show-error --fail --silent https://internal-static.strawlab.org/software/opencv/opencv-4.9.0-static.tar.gz > /tmp/opencv-4.9.0-static.tar.gz
echo "612c071481f37d54552bc8e1d64dc6f89ece1d3140513c582639fe08c1a29cea  /tmp/opencv-4.9.0-static.tar.gz" | sha256sum -c
tar xzf /tmp/opencv-4.9.0-static.tar.gz -C /
