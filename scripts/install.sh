#!/usr/bin/env bash
# One-command, user-level install from source (no sudo).
#   macOS: builds Nilavar.app into ~/Applications and links nilavar-cli into ~/.local/bin
#   Linux: builds and installs into ~/.local (binaries, .desktop entry, icon)
# Requires Rust (https://rustup.rs). The pinned toolchain installs automatically.
set -euo pipefail
cd "$(dirname "$0")/.."

if ! command -v cargo >/dev/null 2>&1; then
  echo "Rust is required: install it from https://rustup.rs and re-run this script." >&2
  exit 1
fi

case "$(uname -s)" in
  Darwin)
    ./scripts/package-macos.sh
    mkdir -p "$HOME/Applications" "$HOME/.local/bin"
    rm -rf "$HOME/Applications/Nilavar.app"
    cp -R dist/Nilavar.app "$HOME/Applications/"
    ln -sf "$HOME/Applications/Nilavar.app/Contents/MacOS/nilavar-cli" "$HOME/.local/bin/nilavar-cli"
    echo
    echo "Installed: ~/Applications/Nilavar.app and ~/.local/bin/nilavar-cli"
    echo "Open it from Finder (Applications in your home folder) or: open ~/Applications/Nilavar.app"
    echo "This local build has an ad-hoc signature only; it is not notarized."
    ;;
  Linux)
    ./scripts/package-linux.sh
    dir="$(ls -d dist/nilavar-*-linux-* | grep -v '\.tar\.gz$' | head -1)"
    "$dir/install.sh"
    ;;
  *)
    echo "On Windows run: pwsh ./scripts/package-windows.ps1  (then unzip dist\\*.zip)" >&2
    exit 1
    ;;
esac
