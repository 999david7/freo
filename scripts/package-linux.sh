#!/bin/sh
# Packages the Linux release build as freo_<version>_amd64.deb and freo-<version>-linux-x86_64.tar.gz
# in dist/. Run on Linux after `cargo build --release` (the release workflow does this on Ubuntu).
set -e
cd "$(dirname "$0")/.."
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
ARCH=$(dpkg --print-architecture 2>/dev/null || echo amd64)
OUT=dist
PKG="$OUT/freo_${VERSION}_${ARCH}"
rm -rf "$OUT"; mkdir -p "$PKG/DEBIAN" "$PKG/usr/bin" "$PKG/usr/share/applications" "$PKG/usr/share/mime/packages" "$PKG/usr/share/doc/freo"
install -m 755 target/release/freo "$PKG/usr/bin/freo"
install -m 755 target/release/freo-mcp "$PKG/usr/bin/freo-mcp"
for s in 32 64 128 256 512; do
  d="$PKG/usr/share/icons/hicolor/${s}x${s}/apps"; mkdir -p "$d"
  ./target/release/make_icon $s "$d/freo.png"
done
install -m 644 README.md "$PKG/usr/share/doc/freo/README.md"
cat > "$PKG/usr/share/applications/freo.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=Freo
GenericName=Parametric CAD
Comment=Creo-style parametric 3D CAD
Exec=freo %f
Icon=freo
Terminal=false
Categories=Graphics;3DGraphics;Engineering;
MimeType=application/x-freo-part;application/x-freo-assembly;application/x-freo-drawing;model/step;model/stl;
DESKTOP
cat > "$PKG/usr/share/mime/packages/freo.xml" <<MIME
<?xml version="1.0" encoding="UTF-8"?>
<mime-info xmlns="http://www.freedesktop.org/standards/shared-mime-info">
  <mime-type type="application/x-freo-part"><comment>Freo part</comment><glob pattern="*.pfprt"/></mime-type>
  <mime-type type="application/x-freo-assembly"><comment>Freo assembly</comment><glob pattern="*.pfasm"/></mime-type>
  <mime-type type="application/x-freo-drawing"><comment>Freo drawing</comment><glob pattern="*.pfdrw"/></mime-type>
</mime-info>
MIME
SIZE=$(du -sk "$PKG/usr" | cut -f1)
cat > "$PKG/DEBIAN/control" <<CONTROL
Package: freo
Version: $VERSION
Section: graphics
Priority: optional
Architecture: $ARCH
Installed-Size: $SIZE
Depends: libc6, libgcc-s1, libgl1, libegl1, libxkbcommon0, libxkbcommon-x11-0, libx11-6, libx11-xcb1, libxcursor1, libxi6, libxrandr2, libwayland-client0, libwayland-egl1, libfontconfig1
Recommends: xdg-desktop-portal, zenity
Maintainer: Freo <noreply@github.com>
Homepage: https://github.com/999david7/freo
Description: Creo-style parametric 3D CAD
 Freo is a history-based parametric 3D modeller that follows the PTC Creo
 workflow: sketcher, feature tree, assemblies, drawings, live simulation and
 an MCP server (freo-mcp) for AI assistants.
CONTROL
dpkg-deb --build --root-owner-group "$PKG" >/dev/null
TAR="freo-${VERSION}-linux-x86_64"
mkdir -p "$OUT/$TAR"
cp target/release/freo target/release/freo-mcp README.md "$OUT/$TAR/"
cp "$PKG/usr/share/applications/freo.desktop" "$OUT/$TAR/"
cp "$PKG/usr/share/icons/hicolor/256x256/apps/freo.png" "$OUT/$TAR/"
tar -C "$OUT" -czf "$OUT/$TAR.tar.gz" "$TAR"
rm -rf "$PKG" "$OUT/$TAR"
ls -la "$OUT"
