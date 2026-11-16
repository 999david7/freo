#!/bin/sh
# Packages the Linux release build in dist/: freo_<version>_<arch>.deb, freo-<version>-1.<arch>.rpm,
# Freo-<version>-<arch>.AppImage and freo-<version>-linux-<arch>.tar.gz. Run on Linux after
# `cargo build --release` (the release workflow does this on Ubuntu, for x86_64 and aarch64).
# The .rpm needs rpmbuild and the AppImage needs APPIMAGETOOL=<path>; without them they are skipped.
set -e
cd "$(dirname "$0")/.."
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
ARCH=$(dpkg --print-architecture 2>/dev/null || echo amd64)
MACHINE=$(uname -m)
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
TAR="freo-${VERSION}-linux-${MACHINE}"
mkdir -p "$OUT/$TAR"
cp target/release/freo target/release/freo-mcp README.md "$OUT/$TAR/"
cp "$PKG/usr/share/applications/freo.desktop" "$OUT/$TAR/"
cp "$PKG/usr/share/icons/hicolor/256x256/apps/freo.png" "$OUT/$TAR/"
tar -C "$OUT" -czf "$OUT/$TAR.tar.gz" "$TAR"
# .rpm (Fedora, openSUSE, RHEL): the same files, library dependencies found by rpmbuild
if command -v rpmbuild >/dev/null 2>&1; then
  TOP="$PWD/$OUT/rpm"; mkdir -p "$TOP/SPECS"
  cat > "$TOP/SPECS/freo.spec" <<SPEC
Name: freo
Version: $VERSION
Release: 1
Summary: Creo-style parametric 3D CAD
License: MIT
URL: https://github.com/999david7/freo
%global debug_package %{nil}
%global __os_install_post %{nil}
%description
Freo is a history-based parametric 3D modeller that follows the PTC Creo
workflow: sketcher, feature tree, assemblies, drawings, live simulation and
an MCP server (freo-mcp) for AI assistants.
%install
mkdir -p %{buildroot}
cp -a "$PWD/$PKG/usr" %{buildroot}/
%files
/usr/bin/freo
/usr/bin/freo-mcp
/usr/share/applications/freo.desktop
/usr/share/mime/packages/freo.xml
/usr/share/icons/hicolor/*/apps/freo.png
/usr/share/doc/freo/README.md
SPEC
  rpmbuild --quiet --define "_topdir $TOP" --define "_build_id_links none" -bb "$TOP/SPECS/freo.spec"
  cp "$TOP"/RPMS/*/*.rpm "$OUT/"
  rm -rf "$TOP"
fi
# AppImage: one file that runs on any distribution (uses the system's graphics libraries)
if [ -n "$APPIMAGETOOL" ]; then
  DIR="$OUT/Freo.AppDir"; mkdir -p "$DIR"
  cp -a "$PKG/usr" "$DIR/"
  cp "$PKG/usr/share/applications/freo.desktop" "$DIR/freo.desktop"
  cp "$PKG/usr/share/icons/hicolor/256x256/apps/freo.png" "$DIR/freo.png"
  printf '#!/bin/sh\nHERE=$(dirname "$(readlink -f "$0")")\nexec "$HERE/usr/bin/freo" "$@"\n' > "$DIR/AppRun"
  chmod +x "$DIR/AppRun"
  ARCH=$MACHINE "$APPIMAGETOOL" --appimage-extract-and-run "$DIR" "$OUT/Freo-${VERSION}-${MACHINE}.AppImage" >/dev/null
  rm -rf "$DIR"
fi
rm -rf "$PKG" "$OUT/$TAR"
ls -la "$OUT"
