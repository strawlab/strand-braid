APPNAME := FlyEye

target/mac/$(APPNAME).app: target/release/fly-eye
	mkdir -p "$@/Contents/MacOS"
	cp -a $< "$@/Contents/MacOS/$(APPNAME)"
	./set-image.py resources/fly.icns "$@"

target/release/fly-eye: src/main.rs
	DC1394_LIBDIR=/Users/straw/devroot/lib cargo build --release
