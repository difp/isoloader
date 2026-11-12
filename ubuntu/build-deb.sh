#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
VERSION="${VERSION:-0.1.0}"
ARCH="$(dpkg --print-architecture)"
STAGE="$ROOT/ubuntu/pkgroot"

cd "$ROOT/cli"
cargo build --release --features desktop --bin isoload-gui
rm -rf "$STAGE"
mkdir -p "$STAGE/usr/bin" "$STAGE/usr/share/applications" "$STAGE/usr/share/metainfo" "$STAGE/DEBIAN"
install -m 0755 target/release/isoload-gui "$STAGE/usr/bin/isoload-gui"
install -m 0644 "$ROOT/ubuntu/isoload-gui.desktop" "$STAGE/usr/share/applications/isoload-gui.desktop"
install -m 0644 "$ROOT/ubuntu/isoload-gui.metainfo.xml" "$STAGE/usr/share/metainfo/org.difp.isoloader.metainfo.xml"
cat > "$STAGE/DEBIAN/control" <<EOF
Package: isoloader
Version: $VERSION
Section: utils
Priority: optional
Architecture: $ARCH
Maintainer: difp <difp@users.noreply.github.com>
Depends: ${DEB_DEPENDS:-libc6, libgcc-s1, libgl1, libx11-6, libxkbcommon0, libwayland-client0}
Description: Create bootable USB drives from ISO images
 ISOLoader writes ISO and IMG images to USB storage devices with optional
 SHA-256 read-back verification.
EOF
dpkg-deb --root-owner-group --build "$STAGE" "$ROOT/ISOLoader_${VERSION}_${ARCH}.deb"
echo "Built $ROOT/ISOLoader_${VERSION}_${ARCH}.deb"
