#!/usr/bin/env bash
# Builds dist/nilavar_<ver>_<arch>.deb for Debian, Ubuntu, Kali and other
# Debian-based distributions. Uses binaries from target/release.
set -euo pipefail
cd "$(dirname "$0")/.."
VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
DEB_VERSION="${VERSION/-/\~}"          # 0.1.0~alpha.1 sorts before 0.1.0
ARCH="$(dpkg --print-architecture)"
[ -x target/release/nilavar ] || cargo build --release --locked -p vault-desktop -p vault-cli
ROOT="$(mktemp -d)/nilavar"
install -Dm755 target/release/nilavar "$ROOT/usr/bin/nilavar"
install -Dm755 target/release/nilavar-cli "$ROOT/usr/bin/nilavar-cli"
install -Dm644 packaging/linux/nilavar.desktop "$ROOT/usr/share/applications/nilavar.desktop"
python3 tools/make_icon.py "$ROOT/usr/share/icons/hicolor/256x256/apps/nilavar.png" 256
install -Dm644 LICENSE "$ROOT/usr/share/doc/nilavar/copyright"
install -Dm644 docs/USER_GUIDE.md "$ROOT/usr/share/doc/nilavar/USER_GUIDE.md"
mkdir -p "$ROOT/DEBIAN"
SIZE="$(du -sk "$ROOT/usr" | cut -f1)"
cat > "$ROOT/DEBIAN/control" <<CONTROL
Package: nilavar
Version: $DEB_VERSION
Section: utils
Priority: optional
Architecture: $ARCH
Installed-Size: $SIZE
Depends: libc6 (>= 2.31), libgcc-s1, libxkbcommon0
Recommends: libvulkan1, mesa-vulkan-drivers | libgl1, libwayland-client0, libx11-6, xdg-desktop-portal
Maintainer: The Nilavar contributors <noreply@users.noreply.github.com>
Homepage: https://github.com/Armur-Ai/nilavar
Description: Offline encrypted workspace for notes and secrets
 Nilavar keeps Markdown notes, passwords, recovery codes and API keys in a
 single age-encrypted vault file. It has no networking code, no accounts and
 no cloud: move the encrypted file between devices and open it with your
 passphrase. Unaudited alpha.
CONTROL
mkdir -p dist
OUT="dist/nilavar_${DEB_VERSION}_${ARCH}.deb"
dpkg-deb --root-owner-group --build "$ROOT" "$OUT"
echo "$OUT"
