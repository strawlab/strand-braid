BUILD_DIR := ../../target/release

# The file names with "VERSION" in them below are wrong (because such files
# do not really exist) unless we update their names it for each build.
# Therefore, these filenames are listed in .PHONY

all: braid-pylon_VERSION_amd64.deb

.PHONY: all braid-pylon_VERSION_amd64.deb

braid-pylon/braid-run:
	# Backwards compat
	cp compat-script braid-pylon/braid-pylon
	chmod a+x braid-pylon/braid-pylon
	chmod go-w braid-pylon/braid-pylon

	# Normal package
	cp $(BUILD_DIR)/braid braid-pylon/braid
	cp $(BUILD_DIR)/braid.changelog braid-pylon/debian/changelog
	cp $(BUILD_DIR)/braid-show-config braid-pylon/
	cp $(BUILD_DIR)/braid-default-config braid-pylon/
	cp $(BUILD_DIR)/braid-run braid-pylon/

braid-pylon_VERSION_amd64.deb: braid-pylon/braid-run
	cd braid-pylon && DEB_BUILD_OPTIONS=nostrip dpkg-buildpackage -rfakeroot -b -uc -us
