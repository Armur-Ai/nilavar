# Security

Nilavar is an **unaudited alpha**. Nothing here is a claim of audited,
certified or "unbreakable" security. This page states what the code does,
what has been tested, and what is out of scope. The detailed analysis is in
[docs/THREAT_MODEL.md](docs/THREAT_MODEL.md).

## Reporting a vulnerability

Please report suspected vulnerabilities privately to the repository
maintainers through the hosting platform's private vulnerability reporting
feature (for example GitHub → *Security* → *Report a vulnerability*). Do not
include real secrets or real vaults in reports — reproduce with synthetic
data. There is currently no bug bounty.

## What protects your data

| Layer | What Nilavar does |
|---|---|
| Encryption at rest | Each vault is one standard **age v1** file in **passphrase (scrypt) mode**, produced by the Rust `age` crate 0.12.1 (which its maintainers label **beta**). Every save generates a fresh file key, scrypt salt and payload nonce. Nilavar implements no cryptographic primitives of its own. |
| Key derivation | scrypt work factor 2^18 (age's reference default) for writing. Files demanding more than 2^20 are rejected before the KDF runs (DoS bound). Measured cost: ~0.31 s per save/open on an Apple M5 Pro. |
| What is encrypted | Everything about the content: titles, folder names, tags, document text, history, trash, vault preferences, ids and timestamps. There is no plaintext index or sidecar file. |
| What is *not* encrypted | The vault file's name, size and filesystem timestamps; the non-sensitive app settings file (UI preferences and, if enabled, the last vault's path); recovery-copy *file names* (timestamps only). See [docs/PRIVACY.md](docs/PRIVACY.md). |
| Integrity | age authenticates the header (HMAC) and every 64 KiB payload chunk (ChaCha20-Poly1305) including the final-chunk flag, so tampering, truncation and reordering are detected. The whole stream is read and authenticated, then the JSON payload is validated (schema, ids, relationships, sizes), before anything reaches the UI. |
| Passphrase handling | Never trimmed or normalized; held in `secrecy::SecretString`; never logged, put in arguments, environment variables or files. Strength guidance is computed locally and is only guidance. |
| Saving | Same-directory temporary file created exclusively with mode 0600, `fsync` (F_FULLFSYNC on Apple platforms), atomic rename, directory sync. The previous file is never truncated first. External modifications are detected by ciphertext fingerprint and never silently overwritten. |
| Memory hygiene | Document text lives in zeroizing buffers that wipe old allocations when they grow; decrypted payloads are staged in pre-sized zeroizing buffers; workspace UI state, editor undo history and layout caches are dropped when locking. Residual copies that cannot be controlled are listed in the threat model. |
| Locking | Manual (`⇧⌘L`), idle (default 5 min), on wake from sleep, when the macOS screen locks, and optionally on focus loss. Automatic locks attempt one encrypted save, then an encrypted recovery copy, and never write plaintext. |
| Clipboard | App-initiated copies (including ⌘C/⌘X in the editor) are cleared after a configurable delay and on lock — only if the clipboard still holds the copied value. Privacy hints exclude them from clipboard history where the OS supports it. Selections are never published to the Linux PRIMARY selection. |
| Network | The application contains **no networking code** (no HTTP client, telemetry, update checks or crash uploads; enforced by a dependency-policy check). On macOS the process additionally applies the OS built-in `no-network` sandbox profile at startup, so the kernel denies all sockets. |
| Crash data | Panic messages are suppressed (only the source location is printed); core dumps are disabled on Unix (`RLIMIT_CORE=0`, plus `PR_SET_DUMPABLE=0` on Linux). |
| Diagnostics | A closed, typed event API that cannot accept strings, paths or document data. Reports are generated only on request, shown for review first, saved locally, never uploaded. |

## Capability matrix

"Implemented" means the code exists; "Tested" means an automated test
exercises it on that platform in this repository's CI or was verified by
hand as noted. Linux and Windows have **not** been manually exercised.

| Protection | macOS | Linux | Windows |
|---|---|---|---|
| No app networking code | Implemented, enforced by dependency policy | same | same |
| OS-enforced network denial | **Implemented & tested** (`sandbox_init` no-network profile; `tests/network_sandbox.rs`; packaged app observed with `lsof`: zero sockets) | Not implemented | Not implemented |
| App Sandbox (file access restriction) | Not used — it would block the same-directory atomic saves next to a user-chosen vault; see docs/DECISIONS.md | n/a | n/a |
| Core dumps disabled | Implemented (`setrlimit`) | Implemented (`setrlimit` + `PR_SET_DUMPABLE`) | Not implemented (Windows Error Reporting is system-configured) |
| Lock on idle | Implemented & tested (UI test) | Implemented | Implemented |
| Lock on sleep (on wake) | Implemented (clock-jump detection) | Implemented (same method) | Implemented (same method; untested) |
| Lock on screen lock | Implemented (CGSession polling) | Not implemented | Not implemented |
| Clipboard privacy hints | Implemented (concealed type / not in history) | Implemented (password-manager hint; honored only by some managers) | Implemented (exclude from history and cloud clipboard) |
| Clipboard auto-clear | Implemented & unit-tested (policy) | same | same |
| File permissions on new files | 0600 files, 0700 data dirs | same | Inherited ACLs of the parent folder (no explicit ACL code) |

## Known limitations (summary)

- No protection against malware or another person using your **unlocked**
  session, against administrator/root attackers, or against screen
  capture/recording while a document is visible. Apps you have granted OS
  accessibility permission can read on-screen text (including the editor)
  through the accessibility tree while the vault is unlocked.
- Freed (non-zeroized) copies can remain in process memory: egui's text
  layout caches, its undo snapshots between lock events, the OS clipboard,
  IME/input-method buffers, GPU textures of rendered glyphs, swap and
  hibernation files. Use full-disk encryption.
- Deleting a file (old vaults, recovery copies, plaintext exports) does not
  guarantee erasure from SSD/flash storage, backups or cloud version
  history. Old copies stay decryptable with the passphrase they were made
  with.
- The local "vault in use" lock only coordinates Nilavar instances on the
  same computer. Cloud sync clients are not coordinated; Nilavar detects
  their changes before saving but a small race window remains.
- Without independently trusted state, a fresh device cannot detect a
  rollback to an older (valid) vault file.

## Development rules

See [CONTRIBUTING.md](CONTRIBUTING.md). In short: synthetic data only, no
secrets in arguments/environment/fixtures, no logging frameworks, no
`dbg!`, run `scripts/check-hygiene.sh` and the full test suite, and use
`gitleaks --redact`. Run untrusted development builds in a separate OS
account or VM; the repository itself cannot enforce host isolation.
