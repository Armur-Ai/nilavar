**Nilavar is an unaudited alpha.** Keep independent backups and read the
[User Guide](https://github.com/Armur-Ai/nilavar/blob/main/docs/USER_GUIDE.md).

| Download | For |
|---|---|
| `Nilavar-macOS-universal.dmg` | macOS 11+ (Apple Silicon and Intel) |
| `Nilavar-Windows-x64.exe` | Windows 10/11 — portable, just run it |
| `Nilavar-Windows-x64.zip` | Windows app + `nilavar-cli` + guide |
| `nilavar_amd64.deb` | Kali, Debian, Ubuntu, Linux Mint, Pop!_OS |
| `nilavar-x86_64.pkg.tar.zst` | Arch Linux, BlackArch, EndeavourOS, Manjaro |
| `nilavar-linux-x86_64.tar.gz` | Any other x86_64 Linux (portable) |
| `SHA256SUMS`, `sbom-cyclonedx.zip` | Checksums and software bill of materials |

**Unsigned builds.** The macOS app has only an ad-hoc signature (not
notarized) and the Windows/Linux builds are unsigned, so your OS will warn on
first launch — the User Guide explains how to open it. Verify downloads with
`SHA256SUMS`.

Nilavar contains no networking code. Your vault is one encrypted file; move
it between devices yourself and open it with the same passphrase.
