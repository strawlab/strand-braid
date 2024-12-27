#!/bin/bash -x
set -o errexit

ORIG_DIR=`pwd`
echo $ORIG_DIR

# Download and install vimba When updating this, also update the file
# ubuntu-*-installer-zip-readme.txt and vimba/src/lib.rs
curl --show-error --fail --silent https://internal-static.strawlab.org/software/vimba/VimbaX_Setup-2024-1-Linux64.tar.gz > /tmp/VimbaX_Setup-2024-1-Linux64.tar.gz
echo "4a77aa2dc0873d0e033b29e71208e6c0603c09a2cc7915c3f2c8c24e54647564 /tmp/VimbaX_Setup-2024-1-Linux64.tar.gz" | sha256sum -c

tar xzf /tmp/VimbaX_Setup-2024-1-Linux64.tar.gz -C /opt

# Now check .so is in the expected location:
ls -l /opt/VimbaX_2024-1/bin/libVmbC.so

cd $ORIG_DIR
