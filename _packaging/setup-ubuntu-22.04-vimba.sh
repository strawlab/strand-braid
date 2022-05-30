#!/bin/bash -x
set -o errexit

ORIG_DIR=`pwd`
echo $ORIG_DIR

# Download and install vimba
# When updating this, also update the file ubuntu-2004-installer-zip-readme.txt and the Pylon version specified in strand-braid/debian/control
curl --show-error --fail --silent https://internal-static.strawlab.org/software/vimba/Vimba_v5.1_Linux64.tgz > /tmp/Vimba_v5.1_Linux64.tgz
echo "1dc990d423aa09c5c340946491fa1a4457f47ebf50d470ff7c6dfe4d90c06e69 /tmp/Vimba_v5.1_Linux64.tgz" | sha256sum -c

mkdir -p /opt/vimba
tar xzf /tmp/Vimba_v5.1_Linux64.tgz -C /opt/vimba

# Now check .so is in the expected location:
ls -l /opt/vimba/Vimba_5_1/VimbaC/DynamicLib/x86_64bit/libVimbaC.so

cd $ORIG_DIR
