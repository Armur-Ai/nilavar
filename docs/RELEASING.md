# Releasing

Releases are **manual** and there are no automatic updates in v1. Any
future auto-update mechanism needs its own reviewed design (signed update
metadata, rollback protection, and an opt-in UI).

## Checklist

1. **Branch is green**: CI (lint, tests on macOS/Linux/Windows, interop,
   secret scan, dependency policy, fuzz smoke) passes on the release commit.
2. `cargo deny check` and `cargo audit` pass; every ignored advisory in
   `deny.toml` still has a valid justification (update docs/DEPENDENCIES.md).
3. Version bumped in the workspace `Cargo.toml`; `Cargo.lock` committed.
4. If the payload schema changed: `SCHEMA_VERSION` bumped, migration
   implemented and tested, docs/FORMAT.md updated.
5. Run the full local suite on macOS including UI flows and interop:
   `cargo test --workspace` with `age` and `rage` installed.
6. Longer fuzz runs (≥ 1 h per target) without new crashes.
7. Regenerate docs/THIRD_PARTY_LICENSES.md (`python3 tools/license_inventory.py`).
8. Manually smoke-test the packaged app with a **synthetic** vault on each
   platform you will publish for: create, edit, save, lock/unlock, history
   restore, export/import, backup/restore, conflict copy. Record which
   platforms were actually tested in the release notes.
9. Push a `vX.Y.Z` tag: the `Release` workflow builds the macOS DMG,
   Windows .exe/.zip, .deb (Kali/Debian/Ubuntu), pacman package
   (Arch/BlackArch) and portable Linux tarball, and publishes them with
   `SHA256SUMS` and SBOMs under stable asset names used by the README buttons.
10. Release notes state: the build is an **unaudited alpha**, what is
    signed and what is not, which platforms were tested, known issues.
11. Tag the release (`git tag -s vX.Y.Z` if you have a signing key).

## Artifacts

| Platform | Script | Output |
|---|---|---|
| macOS | `scripts/package-macos.sh` | `Nilavar-<ver>-macos-<arch>-UNSIGNED.zip` (app bundle), `nilavar-cli-…tar.gz`, `sbom/*.cdx.json`, `SHA256SUMS` |
| Debian/Ubuntu/Kali | `scripts/package-deb.sh` | `nilavar_<ver>_<arch>.deb` |
| Arch/BlackArch | `packaging/arch/PKGBUILD` (source) / `PKGBUILD-bin` (CI) | `nilavar-<ver>-1-x86_64.pkg.tar.zst` |
| Linux | `scripts/package-linux.sh` | `nilavar-<ver>-linux-<arch>-UNSIGNED.tar.gz` (binaries, `.desktop`, icon, user-local `install.sh`) |
| Windows | `scripts/package-windows.ps1` | `nilavar-<ver>-windows-x86_64-UNSIGNED.zip` |

Artifacts are labelled `UNSIGNED` unless signed as below. Never claim a
signature that was not made.

## macOS signing and notarization (maintainer machine only)

Requires an Apple Developer ID certificate in the maintainer's keychain.
Secrets must never be available to pull-request CI jobs; if automated,
use a protected environment with required reviewers.

```bash
codesign --force --options runtime --timestamp \
  --entitlements packaging/macos/Nilavar.entitlements \
  -s "Developer ID Application: <Name> (<TEAMID>)" dist/Nilavar.app/Contents/MacOS/nilavar-cli
codesign --force --options runtime --timestamp \
  --entitlements packaging/macos/Nilavar.entitlements \
  -s "Developer ID Application: <Name> (<TEAMID>)" dist/Nilavar.app
ditto -c -k --keepParent dist/Nilavar.app dist/Nilavar-<ver>-macos.zip
xcrun notarytool submit dist/Nilavar-<ver>-macos.zip --keychain-profile <profile> --wait
xcrun stapler staple dist/Nilavar.app
spctl --assess --type execute -vv dist/Nilavar.app
```

Windows Authenticode signing (`signtool sign /fd sha256 /tr <timestamp-url> /td sha256 …`)
and Linux detached signatures (`gpg --detach-sign SHA256SUMS`) follow the
same rule: maintainer-held keys, never in PR CI.

## Verifying a downloaded release

```bash
shasum -a 256 -c SHA256SUMS          # macOS (sha256sum -c on Linux)
codesign --verify --strict --verbose=2 Nilavar.app   # macOS: ad-hoc or Developer ID
spctl --assess --type execute -vv Nilavar.app        # passes only for notarized builds
```

## Reproducible builds

Nilavar does **not** yet claim reproducible builds. To compare builds
independently:

1. Check out the release tag; use the pinned toolchain
   (`rust-toolchain.toml`) and `cargo build --release --locked`.
2. Build in the same path on the same OS/architecture with
   `SOURCE_DATE_EPOCH` set to the tag's commit time and
   `RUSTFLAGS="--remap-path-prefix=$PWD=/build --remap-path-prefix=$HOME=/home"`.
3. Compare `target/release/nilavar` and `nilavar-cli` with
   `shasum -a 256` (and `diffoscope` if they differ). App bundles differ by
   their ad-hoc signature, so compare the Mach-O binaries before signing.

Until two independent builders have matched hashes, treat release binaries
as trusted only to the extent you trust the builder; building from source
is the alternative.
