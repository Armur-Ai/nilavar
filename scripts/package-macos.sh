#!/usr/bin/env bash
# Builds an UNSIGNED (ad-hoc signed) macOS app bundle, a CLI archive,
# SHA-256 checksums and a CycloneDX SBOM into dist/.
#
# Signing/notarization with a Developer ID is documented in
# docs/RELEASING.md and is never attempted by this script.
set -euo pipefail
cd "$(dirname "$0")/.."

APP_NAME="Nilavar"
BUNDLE_ID="$(sed -n 's/^pub const BUNDLE_ID: &str = "\(.*\)";/\1/p' crates/vault-core/src/app.rs)"
VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
SHORT_VERSION="${VERSION%%-*}"
# UNIVERSAL=1 builds an arm64 + x86_64 universal binary (used for releases).
ARCH="$(uname -m)"
if [ "${UNIVERSAL:-0}" = "1" ]; then ARCH="universal"; fi
DIST="dist"
APP="$DIST/$APP_NAME.app"

echo "==> Building release binaries ($VERSION, $ARCH)"
BIN_DIR="target/release"
if [ "$ARCH" = "universal" ]; then
  rustup target add aarch64-apple-darwin x86_64-apple-darwin >/dev/null
  for t in aarch64-apple-darwin x86_64-apple-darwin; do
    cargo build --release --locked --target "$t" -p vault-desktop -p vault-cli
  done
  BIN_DIR="target/universal-apple-darwin/release"
  mkdir -p "$BIN_DIR"
  for b in nilavar nilavar-cli; do
    lipo -create -output "$BIN_DIR/$b" \
      "target/aarch64-apple-darwin/release/$b" "target/x86_64-apple-darwin/release/$b"
  done
else
  cargo build --release --locked -p vault-desktop -p vault-cli
fi

rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$BIN_DIR/nilavar" "$APP/Contents/MacOS/nilavar"
cp "$BIN_DIR/nilavar-cli" "$APP/Contents/MacOS/nilavar-cli"
sed -e "s/@APP_NAME@/$APP_NAME/g" -e "s/@BUNDLE_ID@/$BUNDLE_ID/g" \
    -e "s/@VERSION@/$VERSION/g" -e "s/@SHORT_VERSION@/$SHORT_VERSION/g" \
    packaging/macos/Info.plist.in > "$APP/Contents/Info.plist"
plutil -lint "$APP/Contents/Info.plist" >/dev/null

echo "==> Generating icon"
ICONSET="$(mktemp -d)/AppIcon.iconset"
mkdir -p "$ICONSET"
for s in 16 32 128 256 512; do
  python3 tools/make_icon.py "$ICONSET/icon_${s}x${s}.png" "$s"
  python3 tools/make_icon.py "$ICONSET/icon_${s}x${s}@2x.png" "$((s * 2))"
done
iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/AppIcon.icns"

echo "==> Ad-hoc signing (NOT a Developer ID signature)"
codesign --force --options runtime --timestamp=none \
  --entitlements packaging/macos/Nilavar.entitlements -s - "$APP/Contents/MacOS/nilavar-cli"
codesign --force --options runtime --timestamp=none \
  --entitlements packaging/macos/Nilavar.entitlements -s - "$APP"
codesign --verify --strict --verbose=2 "$APP"

echo "==> Archiving"
ZIP="$DIST/$APP_NAME-$VERSION-macos-$ARCH-UNSIGNED.zip"
rm -f "$ZIP"
ditto -c -k --sequesterRsrc --keepParent "$APP" "$ZIP"
CLI_TGZ="$DIST/nilavar-cli-$VERSION-macos-$ARCH-UNSIGNED.tar.gz"
tar -C "$BIN_DIR" -czf "$CLI_TGZ" nilavar-cli

echo "==> Disk image"
DMG="$DIST/$APP_NAME-$VERSION-macos-$ARCH-UNSIGNED.dmg"
STAGE="$(mktemp -d)/dmg"
mkdir -p "$STAGE"
cp -R "$APP" "$STAGE/"
ln -s /Applications "$STAGE/Applications"
cp docs/USER_GUIDE.md "$STAGE/User Guide.md" 2>/dev/null || true
rm -f "$DMG"
hdiutil create -volname "$APP_NAME" -srcfolder "$STAGE" -fs HFS+ -format UDZO -ov "$DMG" >/dev/null

echo "==> SBOM"
if command -v cargo-cyclonedx >/dev/null 2>&1; then
  cargo cyclonedx --format json --override-filename "sbom" --all >/dev/null
  mkdir -p "$DIST/sbom"
  for c in vault-core vault-desktop vault-cli; do
    mv "crates/$c/sbom.json" "$DIST/sbom/$c.cdx.json"
  done
else
  echo "   cargo-cyclonedx not installed; skipping SBOM (cargo install cargo-cyclonedx)"
fi

echo "==> Checksums"
( cd "$DIST" && shasum -a 256 ./*.dmg ./*.zip ./*.tar.gz > SHA256SUMS && cat SHA256SUMS )
echo
echo "Done. Artifacts in $DIST/ are UNSIGNED local builds (ad-hoc signature only)."
echo "First launch: right-click $APP_NAME.app → Open, or: xattr -dr com.apple.quarantine $APP"
