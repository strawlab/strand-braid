BUILD_DIR := ../../target/release

# The file names with "VERSION" in them below are wrong (because such files
# do not really exist) unless we update their names it for each build.
# Therefore, these filenames are listed in .PHONY

all: braid-offline_VERSION_amd64.deb

.PHONY: all braid-offline_VERSION_amd64.deb .built

.built:
	cd .. && cargo build --release && cd -
	cd .. && cargo run --release --bin write-debian-changelog -- braid-offline > packaging/changelog && cd -

braid-offline/compute-flydra1-compat: .built
	mkdir -p braid-offline
	cp $(BUILD_DIR)/compute-flydra1-compat $@

braid-offline/offline-retrack: .built
	mkdir -p braid-offline
	cp $(BUILD_DIR)/offline-retrack $@

braid-offline/debian/changelog: .built
	mkdir -p braid-offline/debian
	cp changelog $@

braid-offline_VERSION_amd64.deb: braid-offline/offline-retrack braid-offline/debian/changelog braid-offline/compute-flydra1-compat
	cd braid-offline && DEB_BUILD_OPTIONS=nostrip dpkg-buildpackage -rfakeroot -b -uc -us
