BUILD_DIR := ../../build
STRAND_CAM_PYLON_SRC := $(BUILD_DIR)/strand-cam-pylon
STRAND_CAM_CAMTRIG_PYLON_SRC := $(BUILD_DIR)/strand-cam-camtrig-pylon

# The file names with "VERSION" in them below are wrong (because such files
# do not really exist) unless we update their names it for each build.
# Therefore, these filenames are listed in .PHONY

all: strand-cam-pylon_VERSION_amd64.deb strand-cam-camtrig-pylon_VERSION_amd64.deb

strand-cam-pylon/strand-cam-pylon:
	cp $(STRAND_CAM_PYLON_SRC) strand-cam-pylon/
	cp $(STRAND_CAM_PYLON_SRC).changelog strand-cam-pylon/debian/changelog

strand-cam-pylon_VERSION_amd64.deb: strand-cam-pylon/strand-cam-pylon
	cd strand-cam-pylon && DEB_BUILD_OPTIONS=nostrip dpkg-buildpackage -rfakeroot -b -uc -us

strand-cam-camtrig-pylon/strand-cam-camtrig-pylon:
	cp $(STRAND_CAM_CAMTRIG_PYLON_SRC) strand-cam-camtrig-pylon/
	cp $(BUILD_DIR)/strand-cam-offline-kalmanize strand-cam-camtrig-pylon/
	cp $(STRAND_CAM_CAMTRIG_PYLON_SRC).changelog strand-cam-camtrig-pylon/debian/changelog

strand-cam-camtrig-pylon_VERSION_amd64.deb: strand-cam-camtrig-pylon/strand-cam-camtrig-pylon
	cd strand-cam-camtrig-pylon && DEB_BUILD_OPTIONS=nostrip dpkg-buildpackage -rfakeroot -b -uc -us
