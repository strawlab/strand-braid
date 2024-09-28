#!/bin/bash -x
set -o errexit

apt-get update
DEBIAN_FRONTEND=noninteractive apt-get install -y cpio

ORIG_DIR=`pwd`
echo $ORIG_DIR

# Install IPP
mkdir -p /tmp/download-ipp
cd /tmp/download-ipp
curl -O --show-error --fail --silent https://internal-static.strawlab.org/software/ipp/l_ipp_2019.3.199.tgz
curl -O --show-error --fail --silent https://internal-static.strawlab.org/software/ipp/install-ipp-2019.sh
chmod a+x install-ipp-2019.sh
/tmp/download-ipp/install-ipp-2019.sh
cd /
rm -rf /tmp/download-ipp

cd $ORIG_DIR
