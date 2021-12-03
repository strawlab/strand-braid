#!/bin/bash -x
set -o errexit

ORIG_DIR=`pwd`
echo $ORIG_DIR

# Download and install pylon 6
# When updating this, also update the file ubuntu-2004-installer-zip-readme.txt and the Pylon version specified in strand-braid/debian/control
curl --silent https://internal-static.strawlab.org/software/pylon/pylon_6.2.0.21487-deb0_amd64.deb > /tmp/pylon_6.2.0.21487-deb0_amd64.deb
echo "6acaf99a7331fde2b82217b15642a4e1ae96022bb13c7a91ed1a929ae664e391 /tmp/pylon_6.2.0.21487-deb0_amd64.deb" | sha256sum -c
apt-get install /tmp/pylon_6.2.0.21487-deb0_amd64.deb

cd $ORIG_DIR
