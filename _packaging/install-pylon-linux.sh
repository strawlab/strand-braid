#!/bin/bash -x
set -o errexit

ORIG_DIR=`pwd`
echo $ORIG_DIR

# Download and install pylon
# When updating this, also update the file ubuntu-2004-installer-zip-readme.txt and the Pylon version specified in strand-braid/debian/control
curl --show-error --fail --silent https://internal-static.strawlab.org/software/pylon/pylon_7.3.0.27189-deb0_amd64.deb > /tmp/pylon_7.3.0.27189-deb0_amd64.deb
echo "145d69f0eb081410317a510c0033bd25037e6d2fb51d6caacee560908ab74816  /tmp/pylon_7.3.0.27189-deb0_amd64.deb" | sha256sum -c
apt-get install /tmp/pylon_7.3.0.27189-deb0_amd64.deb

cd $ORIG_DIR
