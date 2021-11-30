BUILD_DIR := ../build

# The file name with "VERSION" in it is wrong (because such files do not really
# exist) unless we update their names it for each build. Therefore, this
# filename is listed in .PHONY

all: strand-braid_VERSION_amd64.deb

.PHONY: all strand-braid_VERSION_amd64.deb

# This does move the file "compute-flydra1-compat", but also all the others.
strand-braid/compute-flydra1-compat:
	mkdir -p strand-braid
	mv -t strand-braid $(BUILD_DIR)/*
	# Move this file back
	mv strand-braid/write-debian-changelog $(BUILD_DIR)

strand-braid/debian/changelog:
	mkdir -p strand-braid/debian
	$(BUILD_DIR)/write-debian-changelog strand-braid focal > strand-braid/debian/changelog

# The full deb
strand-braid_VERSION_amd64.deb: strand-braid/debian/changelog strand-braid/compute-flydra1-compat
	cd strand-braid && DEB_BUILD_OPTIONS=nostrip dpkg-buildpackage -rfakeroot -b -uc -us
