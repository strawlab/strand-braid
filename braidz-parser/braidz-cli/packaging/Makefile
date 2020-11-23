BUILD_DIR := ../../../target/release

# The file names with "VERSION" in them below are wrong (because such files
# do not really exist) unless we update their names it for each build.
# Therefore, these filenames are listed in .PHONY

all: braidz-cli_VERSION_amd64.deb

.PHONY: all braidz-cli_VERSION_amd64.deb .built

.built:
	cd .. && cargo build --release && cd -

braidz-cli/braidz-cli: .built
	mkdir -p braidz-cli
	cp $(BUILD_DIR)/braidz-cli $@

braidz-cli_VERSION_amd64.deb: braidz-cli/braidz-cli
	cd braidz-cli && DEB_BUILD_OPTIONS=nostrip dpkg-buildpackage -rfakeroot -b -uc -us
