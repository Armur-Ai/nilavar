# Dependencies

Versions are locked in `Cargo.lock`; the toolchain is pinned in
`rust-toolchain.toml` (Rust 1.99.0). The full crate/license list is in
[THIRD_PARTY_LICENSES.md](THIRD_PARTY_LICENSES.md).

## Cryptography

| Crate | Version | Use | Status |
|---|---|---|---|
| `age` | 0.12.1 | All encryption and decryption (scrypt passphrase recipient, STREAM payload) | Published by its maintainers as **`[BETA]`**. Default features off (no plugins, SSH, armor or CLI support). Interoperability with reference `age` 1.3.2 and `rage` 0.12.1 is tested in both directions. |
| `age-core` | 0.12.0 | Pulled in by `age`; its `secrecy` 0.10 re-export is what Nilavar uses for passphrases (`age::secrecy`) so versions cannot drift | Beta, same maintainers |
| `zeroize` | 1.9 | Zeroizing buffers for plaintext | Stable |
| `sha2` | 0.10 | Ciphertext fingerprints, recovery-dir and lock-file names, keyed clipboard digest | Stable (same major as age's) |
| `getrandom` | 0.4 | Random ids and passphrase suggestions (OS CSPRNG) | Stable |

`age` 0.12 also links P-256/HPKE code for its `tag`/`tagpq` recipient types
via `elliptic-curve` 0.13 → `crypto-bigint` 0.5. Nilavar never constructs
those recipients and refuses non-passphrase files before decryption, so
that code does not run.

## Accepted advisories

Each ignore in `deny.toml` / `.cargo/audit.toml`, re-checked 2026-11-14:

| Advisory | Crate | Why accepted |
|---|---|---|
| RUSTSEC-2026-0387 (timing variability) | `crypto-bigint` 0.5.5 | Reached only through age's unused P-256 recipient code (see above). No upgrade path while `age` 0.12.1 pins `elliptic-curve` 0.13. Re-evaluate on every `age` update. |
| RUSTSEC-2026-0374 (unmaintained) | `basic-toml` | Build-time proc-macro dependency of age's i18n message embedding. |
| RUSTSEC-2026-0192 (unmaintained) | `ttf-parser` (via `sctk-adwaita`) | Linux/Wayland client-side window decorations from `winit`. No fixed alternative upstream yet. |
| RUSTSEC-2026-0370/0371/0372 (unmaintained) | `zvariant`, `zbus_names`, `zvariant_derive` | Linux accessibility (AT-SPI) stack from `accesskit`; the crates were merged into `zbus` upstream. |

## GUI and platform

| Crate | Notes |
|---|---|
| `eframe`/`egui` 0.37 | Default features (wgpu renderer, winit, AccessKit accessibility, clipboard, `links`, system fonts, x11/wayland). **`persistence` is not enabled**, so egui never serializes widget state (which can include text). The `links` feature only opens a URL after the explicit confirmation dialog. |
| `rfd` 0.18 | Native file dialogs (default features; xdg-portal on Linux). |
| `arboard` 3.7 | Clipboard with privacy hints (default features off: no image support). |
| `pulldown-cmark` 0.13 | Markdown parsing for the inert preview (default features off: no HTML renderer). |
| `similar` 3 | Line diffs for history (bounded by a timeout). |
| `objc2-core-graphics`/`objc2-core-foundation` 0.3 (macOS) | Screen-lock state via `CGSessionCopyCurrentDictionary`. |
| `libc` 0.2 (Unix) | `setrlimit`, `prctl` for disabling core dumps. |
| `clap` 4, `rpassword` 7 | CLI parsing and no-echo passphrase prompts. |

There are **no HTTP, TLS or async network runtime crates** in the shipped
dependency graph; `deny.toml` bans them and `scripts/check-hygiene.sh`
checks the tree. (`tokio` appears in `Cargo.lock` only for `wasm32`
targets, which are not built.) `url` and `webbrowser` come from egui's
`links` feature and only hand a URL to the system browser.

## Unsafe code

First-party code: `vault-core` forbids `unsafe`. `vault-desktop` and
`vault-cli` deny it except in documented functions:
`platform::disable_core_dumps` (`setrlimit`/`prctl`),
`platform::deny_network` and the CLI's equivalent (`sandbox_init`), and one
type cast in `platform::screen_locked`. Each has a `// SAFETY:` comment.

Dependencies **do** contain unsafe code — notably the cryptographic
primitives under `age` (RustCrypto), `wgpu`/`winit`/`objc2` platform
bindings, `arboard` and `zeroize` itself. Nilavar relies on those crates'
own review and testing; this project has not audited them.

## Bundled assets

| Asset | License |
|---|---|
| JetBrains Mono 2.304 (Regular, Bold) | SIL OFL 1.1 — PUA cmap entries removed, see `assets/fonts/NOTICE.md` |
| Inter 4.1 (Regular, SemiBold) | SIL OFL 1.1 — PUA cmap entries removed |
| Phosphor Icons (regular) | MIT |
| EFF Large Wordlist | CC BY 3.0 US |
| egui's default fonts (Ubuntu-Light, Hack, Noto Emoji, emoji-icon-font) | Their respective open licenses, as distributed with egui |
