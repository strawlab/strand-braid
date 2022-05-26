#!/bin/bash -x
set -o errexit

curl --show-error --fail --silent https://internal-static.strawlab.org/software/opencv/opencv-4.5.5-static.tar.gz > /tmp/opencv-4.5.5-static.tar.gz
echo "6dfc8bed523fd1833beb2bdde264863dc4cf49670e635bc987f01fd85638a7e6  /tmp/opencv-4.5.5-static.tar.gz" | sha256sum -c
tar xzf /tmp/opencv-4.5.5-static.tar.gz -C /
