#!/bin/bash -x
set -o errexit

# Download pylon and install 5
curl --silent https://internal-static.strawlab.org/software/pylon/pylon_5.2.0.13457-deb0_amd64.deb > /tmp/pylon_5.2.0.13457-deb0_amd64.deb
echo "9d4f70aae93012d6ca21bb4aff706ce409da155a446e86b90d00f0dd0a26fd55  /tmp/pylon_5.2.0.13457-deb0_amd64.deb" | sha256sum -c
apt-get install -y --allow-downgrades /tmp/pylon_5.2.0.13457-deb0_amd64.deb
