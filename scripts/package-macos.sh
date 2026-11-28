#!/usr/bin/env sh
# Packages the release build into SpeakWrite.app and a disk image to drag it into Applications:
#   ./scripts/build.sh && ./scripts/package-macos.sh [output.dmg]
# Default output: target/release/bundle/SpeakWrite.dmg.
#
# The app is signed ad hoc only (no Apple developer certificate): Apple silicon refuses to run
# unsigned code, and a bundle needs a signature of its own once its files are assembled.
set -eu
cd "$(dirname "$0")/.."

binary=target/release/speakwrite
[ -x "$binary" ] || { echo "missing $binary: run ./scripts/build.sh first" >&2; exit 1; }
version=$(cargo pkgid -p speakwrite | sed 's/.*@//')
bundle=target/release/bundle
dmg=${1:-$bundle/SpeakWrite.dmg}

rm -rf "$bundle"
app="$bundle/stage/SpeakWrite.app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$binary" "$app/Contents/MacOS/speakwrite"
sed "s/@VERSION@/$version/g" packaging/macos/Info.plist > "$app/Contents/Info.plist"
plutil -lint "$app/Contents/Info.plist" >/dev/null
cp LICENSE "$app/Contents/Resources/"
codesign --force --sign - "$app"
codesign --verify --strict "$app"

# The image shows the app next to a link to Applications.
ln -s /Applications "$bundle/stage/Applications"
mkdir -p "$(dirname "$dmg")"
hdiutil create -quiet -volname "SpeakWrite $version" -srcfolder "$bundle/stage" -ov -format UDZO "$dmg"
echo "$dmg"
