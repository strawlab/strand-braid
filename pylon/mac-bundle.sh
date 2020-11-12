#!/bin/bash -x
set -o errexit

# http://bazaar.launchpad.net/~widelands-dev/widelands/trunk/view/head:/utils/macos/build_app.sh#L47

DESTINATION=/tmp/one-build
EXECUTABLE=$DESTINATION/one.app/Contents/MacOS/one
SOURCE_DIR=`pwd`

function MakeAppPackage {
   echo "Making $DESTINATION/one.app now."
   rm -Rf $DESTINATION/

   mkdir $DESTINATION/
   mkdir $DESTINATION/one.app/
   mkdir $DESTINATION/one.app/Contents/
   mkdir $DESTINATION/one.app/Contents/Resources/
   mkdir $DESTINATION/one.app/Contents/MacOS/

   mkdir $DESTINATION/one.app/Contents/Frameworks
   cp -a /Library/Frameworks/pylon.framework $DESTINATION/one.app/Contents/Frameworks/

   cp $SOURCE_DIR/fly.icns $DESTINATION/one.app/Contents/Resources/one.icns
   ln -s /Applications $DESTINATION/Applications

   cat > $DESTINATION/one.app/Contents/Info.plist << EOF
{
   CFBundleName = one;
   CFBundleDisplayName = one;
   CFBundleIdentifier = "org.strawlab.one";
   CFBundleVersion = "1.0";
   CFBundleInfoDictionaryVersion = "6.0";
   CFBundlePackageType = APPL;
   CFBundleSignature = pone;
   CFBundleExecutable = one;
   CFBundleIconFile = "fly.icns";
}
EOF
   echo "Copying binary ..."
   cp -a ../target/debug/examples/one $DESTINATION/one.app/Contents/MacOS/

   echo "Stripping binary ..."
   strip -u -r $DESTINATION/one.app/Contents/MacOS/one
}

# This is not the right way to build a bundle. The bundle should copy the framework inside it
# and recompile the app to link the bundle relative to @executable_path
MakeAppPackage
