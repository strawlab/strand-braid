BUILD_DIR := ../../build
STRAND_CAM_PYLON_SRC := $(BUILD_DIR)/strand-cam-pylon
STRAND_CAM_led_box_PYLON_SRC := $(BUILD_DIR)/strand-cam-led-box-pylon

# The file names with "VERSION" in them below are wrong (because such files
# do not really exist) unless we update their names it for each build.
# Therefore, these filenames are listed in .PHONY

all: strand-cam-pylon_VERSION_amd64.deb strand-cam-led-box-pylon_VERSION_amd64.deb

strand-cam-pylon/strand-cam-pylon:
	cp $(STRAND_CAM_PYLON_SRC) strand-cam-pylon/
	cp $(STRAND_CAM_PYLON_SRC).changelog strand-cam-pylon/debian/changelog

strand-cam-pylon_VERSION_amd64.deb: strand-cam-pylon/strand-cam-pylon
	cd strand-cam-pylon && DEB_BUILD_OPTIONS=nostrip dpkg-buildpackage -rfakeroot -b -uc -us

strand-cam-led-box-pylon/strand-cam-led-box-pylon:
	cp $(STRAND_CAM_led_box_PYLON_SRC) strand-cam-led-box-pylon/
	cp $(BUILD_DIR)/strand-cam-offline-kalmanize strand-cam-led-box-pylon/
	cp $(STRAND_CAM_led_box_PYLON_SRC).changelog strand-cam-led-box-pylon/debian/changelog

strand-cam-led-box-pylon_VERSION_amd64.deb: strand-cam-led-box-pylon/strand-cam-led-box-pylon
	cd strand-cam-led-box-pylon && DEB_BUILD_OPTIONS=nostrip dpkg-buildpackage -rfakeroot -b -uc -us
