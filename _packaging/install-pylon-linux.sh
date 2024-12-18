#!/bin/bash -x
set -o errexit

ORIG_DIR=`pwd`
echo $ORIG_DIR

ARCH=`arch`

# Download and install pylon
if [[ $ARCH == x86_64 ]]; then

    # When updating this, also update the file ubuntu-2x04-installer-zip-readme.txt and the Pylon version specified in strand-braid/debian/control

    curl --show-error --fail --silent https://internal-static.strawlab.org/software/pylon/pylon_7.3.0.27189-deb0_amd64.deb > /tmp/pylon_7.3.0.27189-deb0_amd64.deb
    echo "145d69f0eb081410317a510c0033bd25037e6d2fb51d6caacee560908ab74816  /tmp/pylon_7.3.0.27189-deb0_amd64.deb" | sha256sum -c
    apt-get install /tmp/pylon_7.3.0.27189-deb0_amd64.deb
elif [[ $ARCH == aarch64 ]]; then
    curl --show-error --fail --silent https://internal-static.strawlab.org/software/pylon/pylon_6.2.0.21487-deb0_arm64.deb > /tmp/pylon_6.2.0.21487-deb0_arm64.deb
    echo "327c6f70e4bd5aa8c3afee924e4d0f74008c0939d32e56219fa2df3749944372" /tmp/pylon_6.2.0.21487-deb0_arm64.deb | sha256sum -c
    apt-get install /tmp/pylon_6.2.0.21487-deb0_arm64.deb
else
    echo "ERROR: unknown architecture"
    exit 1
fi

cd $ORIG_DIR
