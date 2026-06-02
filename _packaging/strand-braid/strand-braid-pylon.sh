# strand-braid: point pylon-shimload at the bundled Basler Pylon C-ABI shim.
#
# The strand-braid package installs a versioned shim under
# /usr/lib/strand-braid/ together with a stable libpylon-cabi.so symlink.
# pylon-shimload loads the shared library named by the PYLON_CABI environment
# variable, so we default it to the bundled shim here.
#
# To run against a different Pylon version, download (or build) the matching
# shim, install the matching Basler Pylon runtime, and export
# PYLON_CABI=/path/to/that/shim before launching. An already-set PYLON_CABI is
# left untouched below, so such an override always wins.
if [ -z "${PYLON_CABI:-}" ] && [ -e /usr/lib/strand-braid/libpylon-cabi.so ]; then
    PYLON_CABI=/usr/lib/strand-braid/libpylon-cabi.so
    export PYLON_CABI
fi
