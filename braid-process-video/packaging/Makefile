BUILD_DIR := ../../target/release

# The file names with "VERSION" in them below are wrong (because such files
# do not really exist) unless we update their names it for each build.
# Therefore, these filenames are listed in .PHONY

all:braid-process-video_VERSION_amd64.deb

.PHONY: allbraid-process-video_VERSION_amd64.deb .built

.built:
	cd .. && cargo build --release && cd -

braid-process-video/braid-process-video: .built
	mkdir -p braid-process-video
	cp $(BUILD_DIR)/braid-process-video $@

braid-process-video_VERSION_amd64.deb:braid-process-video/braid-process-video braid-process-video/debian/changelog
	cd braid-process-video && DEB_BUILD_OPTIONS=nostrip dpkg-buildpackage -rfakeroot -b -uc -us
