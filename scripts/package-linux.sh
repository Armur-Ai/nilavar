#!/usr/bin/env bash
# Builds an unsigned Linux tarball (app + CLI + .desktop + icon), SHA-256
# checksums and SBOMs into dist/. Install by extracting and running
# ./install.sh (user-local, no root).
set -euo pipefail
cd "$(dirname "$0")/.."
VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
ARCH="$(uname -m)"
NAME="nilavar-$VERSION-linux-$ARCH"
OUT="dist/$NAME"
cargo build --release --locked -p vault-desktop -p vault-cli
rm -rf "$OUT" && mkdir -p "$OUT"
cp target/release/nilavar target/release/nilavar-cli "$OUT/"
cp packaging/linux/nilavar.desktop "$OUT/"
python3 tools/make_icon.py "$OUT/nilavar.png" 256
cp LICENSE README.md "$OUT/"
cp -r docs "$OUT/docs"
cat > "$OUT/install.sh" <<'INSTALL'
#!/usr/bin/env bash
# User-local install (no root): ~/.local/bin, ~/.local/share/applications, icons.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
install -Dm755 "$here/nilavar" "$HOME/.local/bin/nilavar"
install -Dm755 "$here/nilavar-cli" "$HOME/.local/bin/nilavar-cli"
install -Dm644 "$here/nilavar.desktop" "$HOME/.local/share/applications/nilavar.desktop"
install -Dm644 "$here/nilavar.png" "$HOME/.local/share/icons/hicolor/256x256/apps/nilavar.png"
echo "Installed to ~/.local/bin. Ensure ~/.local/bin is on your PATH."
INSTALL
chmod +x "$OUT/install.sh"
tar -C dist -czf "dist/$NAME-UNSIGNED.tar.gz" "$NAME"
if command -v cargo-cyclonedx >/dev/null 2>&1; then
  cargo cyclonedx --format json --override-filename sbom --all >/dev/null
  mkdir -p dist/sbom
  for c in vault-core vault-desktop vault-cli; do mv "crates/$c/sbom.json" "dist/sbom/$c.cdx.json"; done
fi
( cd dist && sha256sum ./*.tar.gz > SHA256SUMS && cat SHA256SUMS )
