APPNAME := fview2

../target/mac/$(APPNAME).app: ../target/release/fview2
	mkdir -p "$@/Contents/MacOS"
	cp -a $< "$@/Contents/MacOS/$(APPNAME)"
	install_name_tool -add_rpath @loader_path/.. "$@/Contents/MacOS/$(APPNAME)"
	# mkdir -p target/mac/fview2.app/Frameworks
  	# cp -a /Library/Frameworks/pylon.framework target/mac/fview2.app/Frameworks
	./set-image.py resources/fly.icns "$@"

../target/release/fview2: src/main.rs
	DC1394_LIBDIR="/Users/straw/devroot/lib" cargo build --release --no-default-features --features "serve_files pylon"
