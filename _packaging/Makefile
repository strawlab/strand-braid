BUILD_DIR := ../build

# Ubuntu release codename written into debian/changelog (the `Distribution`
# field). Callers (e.g. the GitHub Actions packaging job) override this to match
# the Ubuntu version being built; it defaults to focal for standalone use.
DISTRIBUTION ?= focal

# The file name with "VERSION" in it is wrong (because such files do not really
# exist) unless we update their names it for each build. Therefore, this
# filename is listed in .PHONY

all: strand-braid_VERSION_amd64.deb

.PHONY: all strand-braid_VERSION_amd64.deb

# Move all binaries (except write-debian-changelog) from ../build/ to
# ./strand-braid/ (This has "strand-cam" as the target, but also all
# other files brought along.)
strand-braid/strand-cam:
	mkdir -p strand-braid
	mv -t strand-braid $(BUILD_DIR)/*
	# Move this file back
	mv strand-braid/write-debian-changelog $(BUILD_DIR)

strand-braid/debian/changelog:
	mkdir -p strand-braid/debian
	$(BUILD_DIR)/write-debian-changelog strand-braid $(DISTRIBUTION) > strand-braid/debian/changelog

# The full deb
strand-braid_VERSION_amd64.deb: strand-braid/debian/changelog strand-braid/strand-cam
	cd strand-braid && dpkg-buildpackage -rfakeroot -b -uc -us
