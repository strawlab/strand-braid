BUILD_DIR := ../build

# The file name with "VERSION" in it is wrong (because such files do not really
# exist) unless we update their names it for each build. Therefore, this
# filename is listed in .PHONY

all: strand-braid_VERSION_amd64.deb

.PHONY: all strand-braid_VERSION_amd64.deb

# Move all binaries (except write-debian-changelog) from ../build/ to
# ./strand-braid/ (This has "strand-cam-pylon" as the target, but also all
# other files brought along.)
strand-braid/strand-cam-pylon:
	mkdir -p strand-braid
	mv -t strand-braid $(BUILD_DIR)/*
	# Move this file back
	mv strand-braid/write-debian-changelog $(BUILD_DIR)

strand-braid/debian/changelog:
	mkdir -p strand-braid/debian
	$(BUILD_DIR)/write-debian-changelog strand-braid focal > strand-braid/debian/changelog

# The full deb
strand-braid_VERSION_amd64.deb: strand-braid/debian/changelog strand-braid/strand-cam-pylon
	cd strand-braid && dpkg-buildpackage -rfakeroot -b -uc -us
