BUILD_DIR := ../../build
FVIEW2_DC1394_SRC := $(BUILD_DIR)/strand-cam-dc1394
FVIEW2_PYLON_SRC := $(BUILD_DIR)/strand-cam-pylon
FVIEW2_CAMTRIG_PYLON_SRC := $(BUILD_DIR)/strand-cam-camtrig-pylon

# The file names with "VERSION" in them below are wrong (because such files
# do not really exist) unless we update their names it for each build.
# Therefore, these filenames are listed in .PHONY

all: strand-cam-dc1394_VERSION_amd64.deb strand-cam-pylon_VERSION_amd64.deb \
	strand-cam-camtrig-pylon_VERSION_amd64.deb

.PHONY: all strand-cam-dc1394_VERSION_amd64.deb strand-cam-pylon_VERSION_amd64.deb \
	strand-cam-camtrig-pylon_VERSION_amd64.deb

strand-cam-dc1394/strand-cam-dc1394:
	cp $(FVIEW2_DC1394_SRC) strand-cam-dc1394/
	cp $(FVIEW2_DC1394_SRC).changelog strand-cam-dc1394/debian/changelog

strand-cam-dc1394_VERSION_amd64.deb: strand-cam-dc1394/strand-cam-dc1394
	cd strand-cam-dc1394 && DEB_BUILD_OPTIONS=nostrip dpkg-buildpackage -rfakeroot -b -uc -us

strand-cam-pylon/strand-cam-pylon:
	cp $(FVIEW2_PYLON_SRC) strand-cam-pylon/
	cp $(FVIEW2_PYLON_SRC).changelog strand-cam-pylon/debian/changelog

strand-cam-pylon_VERSION_amd64.deb: strand-cam-pylon/strand-cam-pylon
	cd strand-cam-pylon && DEB_BUILD_OPTIONS=nostrip dpkg-buildpackage -rfakeroot -b -uc -us

strand-cam-camtrig-pylon/strand-cam-camtrig-pylon:
	cp $(FVIEW2_CAMTRIG_PYLON_SRC) strand-cam-camtrig-pylon/
	cp $(BUILD_DIR)/strand-cam-offline-kalmanize strand-cam-camtrig-pylon/
	cp $(FVIEW2_CAMTRIG_PYLON_SRC).changelog strand-cam-camtrig-pylon/debian/changelog

strand-cam-camtrig-pylon_VERSION_amd64.deb: strand-cam-camtrig-pylon/strand-cam-camtrig-pylon
	cd strand-cam-camtrig-pylon && DEB_BUILD_OPTIONS=nostrip dpkg-buildpackage -rfakeroot -b -uc -us
