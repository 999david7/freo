#!/bin/sh
# Builds Freo.app (release) in the project root.
set -e
cd "$(dirname "$0")/.."
cargo build --release
APP=Freo.app
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp target/release/freo "$APP/Contents/MacOS/freo"
ICONSET=target/Freo.iconset
rm -rf "$ICONSET"; mkdir -p "$ICONSET"
for s in 16 32 64 128 256 512 1024; do ./target/release/make_icon $s "$ICONSET/tmp_$s.png"; done
cp "$ICONSET/tmp_16.png" "$ICONSET/icon_16x16.png"
cp "$ICONSET/tmp_32.png" "$ICONSET/icon_16x16@2x.png"
cp "$ICONSET/tmp_32.png" "$ICONSET/icon_32x32.png"
cp "$ICONSET/tmp_64.png" "$ICONSET/icon_32x32@2x.png"
cp "$ICONSET/tmp_128.png" "$ICONSET/icon_128x128.png"
cp "$ICONSET/tmp_256.png" "$ICONSET/icon_128x128@2x.png"
cp "$ICONSET/tmp_256.png" "$ICONSET/icon_256x256.png"
cp "$ICONSET/tmp_512.png" "$ICONSET/icon_256x256@2x.png"
cp "$ICONSET/tmp_512.png" "$ICONSET/icon_512x512.png"
cp "$ICONSET/tmp_1024.png" "$ICONSET/icon_512x512@2x.png"
rm "$ICONSET"/tmp_*.png
iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/Freo.icns"
cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleName</key><string>Freo</string>
  <key>CFBundleDisplayName</key><string>Freo</string>
  <key>CFBundleIdentifier</key><string>io.github.freo</string>
  <key>CFBundleVersion</key><string>0.1.0</string>
  <key>CFBundleShortVersionString</key><string>0.1.0</string>
  <key>CFBundleExecutable</key><string>freo</string>
  <key>CFBundleIconFile</key><string>Freo</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>CFBundleDocumentTypes</key><array><dict>
    <key>CFBundleTypeName</key><string>Freo Part</string>
    <key>CFBundleTypeExtensions</key><array><string>pfprt</string></array>
    <key>CFBundleTypeRole</key><string>Editor</string>
  </dict><dict>
    <key>CFBundleTypeName</key><string>Freo Assembly</string>
    <key>CFBundleTypeExtensions</key><array><string>pfasm</string></array>
    <key>CFBundleTypeRole</key><string>Editor</string>
  </dict><dict>
    <key>CFBundleTypeName</key><string>Freo Drawing</string>
    <key>CFBundleTypeExtensions</key><array><string>pfdrw</string></array>
    <key>CFBundleTypeRole</key><string>Editor</string>
  </dict><dict>
    <key>CFBundleTypeName</key><string>STEP Model</string>
    <key>CFBundleTypeExtensions</key><array><string>stp</string><string>step</string><string>p21</string></array>
    <key>CFBundleTypeRole</key><string>Viewer</string>
    <key>LSHandlerRank</key><string>Alternate</string>
  </dict><dict>
    <key>CFBundleTypeName</key><string>Mesh Model</string>
    <key>CFBundleTypeExtensions</key><array><string>stl</string><string>obj</string></array>
    <key>CFBundleTypeRole</key><string>Viewer</string>
    <key>LSHandlerRank</key><string>Alternate</string>
  </dict><dict>
    <key>CFBundleTypeName</key><string>Creo Model</string>
    <key>CFBundleTypeExtensions</key><array><string>prt</string><string>asm</string><string>drw</string></array>
    <key>CFBundleTypeRole</key><string>Viewer</string>
    <key>LSHandlerRank</key><string>Alternate</string>
  </dict></array>
</dict></plist>
PLIST
echo "Built $APP"
