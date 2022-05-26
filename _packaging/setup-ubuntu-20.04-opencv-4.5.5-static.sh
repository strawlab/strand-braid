#!/bin/bash -x
set -o errexit

# Note: the software in this repository that requires opencv also work with
# opencv 4 as packaged by Ubuntu and Debian (and presumably other linuxes).
# Install this with `apt-get install libopencv-dev`. In our continuous
# integration system (for which this script is built), it is helpful to keep the
# setup time of the build environment to a minimum and thus we use our own
# packaging of opencv. This is substantially faster to install.

curl --show-error --fail --silent https://internal-static.strawlab.org/software/opencv/opencv-4.5.5-static.tar.gz > /tmp/opencv-4.5.5-static.tar.gz
echo "6dfc8bed523fd1833beb2bdde264863dc4cf49670e635bc987f01fd85638a7e6  /tmp/opencv-4.5.5-static.tar.gz" | sha256sum -c
tar xzf /tmp/opencv-4.5.5-static.tar.gz -C /
