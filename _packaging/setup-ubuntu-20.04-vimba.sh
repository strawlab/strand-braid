#!/bin/bash -x
set -o errexit

ORIG_DIR=`pwd`
echo $ORIG_DIR

# Download and install vimba
# When updating this, also update the file ubuntu-2004-installer-zip-readme.txt
curl --show-error --fail --silent https://internal-static.strawlab.org/software/vimba/Vimba64_v6.0_Linux.tgz > /tmp/Vimba64_v6.0_Linux.tgz
echo "48892d6657c07fe410e627f96ea6ea22c2aeab4f08010a1de25a25a1a19e275c /tmp/Vimba64_v6.0_Linux.tgz" | sha256sum -c

mkdir -p /opt/vimba
tar xzf /tmp/Vimba64_v6.0_Linux.tgz -C /opt/vimba

# Now check .so is in the expected location:
ls -l /opt/vimba/Vimba_6_0/VimbaC/DynamicLib/x86_64bit/libVimbaC.so

cd $ORIG_DIR
