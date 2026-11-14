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
ARCH="$(uname -m)"
DIST="dist"
APP="$DIST/$APP_NAME.app"

echo "==> Building release binaries ($VERSION, $ARCH)"
cargo build --release --locked -p vault-desktop -p vault-cli

rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp target/release/nilavar "$APP/Contents/MacOS/nilavar"
cp target/release/nilavar-cli "$APP/Contents/MacOS/nilavar-cli"
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
tar -C target/release -czf "$CLI_TGZ" nilavar-cli

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
( cd "$DIST" && shasum -a 256 ./*.zip ./*.tar.gz > SHA256SUMS && cat SHA256SUMS )
echo
echo "Done. Artifacts in $DIST/ are UNSIGNED local builds (ad-hoc signature only)."
echo "First launch: right-click $APP_NAME.app → Open, or: xattr -dr com.apple.quarantine $APP"
