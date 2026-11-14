# Threat model

Scope: Nilavar `0.1.0-alpha.1` (unaudited). This document describes what
the design tries to protect, against whom, and where protection ends.

## Assets

1. **Vault plaintext**: document text, titles, folder names, tags, history,
   trash, vault preferences.
2. **The passphrase**.
3. **Availability of the data**: the ability to open the vault later,
   possibly on a different computer, after crashes, full disks or conflicts.
4. **Metadata**: which documents exist, how many, how large, when edited.

## Adversaries and assumptions

| Adversary | In scope? | Expected outcome |
|---|---|---|
| **A1. Someone who obtains a copy of the vault file** (lost USB stick, compromised cloud account, backup leak) | Yes — primary | Learns only the file's name, size and timestamps. Must guess the passphrase offline against scrypt (2^18). Strength depends entirely on the passphrase; Nilavar shows a local estimate and offers random seven-word passphrases (~90 bits). |
| **A2. Someone who tampers with the vault file** (cloud provider, malware without code execution in the app, bit rot) | Yes | Tampering, truncation, chunk reordering and splicing are detected by age's authentication; the app refuses to open the file and never partially loads it. A *valid older* copy (rollback) cannot be detected on a fresh device. |
| **A3. A malicious vault or import file** (crafted to crash or exhaust resources) | Yes | Size limits before allocation, a hard scrypt work-factor cap (2^20), JSON nesting pre-scan, strict schema (unknown fields/schemas rejected), full validation. Fuzzed. |
| **A4. Another program or device writing the same vault** (sync clients, a second app instance, another computer) | Yes | Same-machine instances are excluded by a local lock. Other writers are detected via ciphertext fingerprint before saving; the user gets an encrypted conflict copy, never a silent overwrite or merge. |
| **A5. Crashes, power loss, full disks** | Yes | Atomic same-directory write with fsync; the previous file is never truncated; encrypted last-known-good copies; edits stay in memory when a save fails. |
| **A6. Passive local observers after the app is locked or closed** (someone with your user account later, a support bundle, a forensic look at app data) | Partly | No plaintext caches, indexes, logs or emergency files are written by the app. Settings contain no content. Residual OS-level copies may exist (see below). |
| **A7. Someone using your unlocked session** (shoulder surfing, an unattended unlocked laptop) | Partly | Idle, sleep and screen-lock auto-lock with immediate masking; clipboard auto-clear. Nothing protects what they can see or copy while it is unlocked. |
| **A8. Malware running as your user, or root/administrator** | **No** | Such an attacker can read process memory, log keystrokes, capture the screen, replace the app binary or read the passphrase as you type it. |
| **A9. Network attackers** | Mostly N/A | The app implements no networking. On macOS the kernel additionally denies sockets for the process. Opening a link from the preview (only after an explicit confirmation showing the destination) hands the URL to your browser, which is outside Nilavar. |
| **A10. Supply chain** (malicious dependency or build tool) | Partly | Locked dependency versions, `cargo deny`/`cargo audit` in CI, no network-capable crates allowed, SBOMs per build. No reproducible-build guarantee yet (see RELEASING.md). |

## Trust boundaries

- **The age file** is the only persistent location of content. The JSON
  payload is parsed only after the entire age stream has been authenticated.
- **The worker thread** performs scrypt/encryption off the UI thread. Its
  queue holds at most one job; results from before a lock are discarded
  (sessions they carry are dropped and zeroized) using a lock epoch.
- **The GUI** (egui) receives plaintext only while unlocked.
- **The OS** (clipboard, file dialogs, input methods, compositor, swap) is
  trusted to the extent listed below.

## Residual plaintext copies (known, not fully controllable)

| Where | Why | Mitigation |
|---|---|---|
| egui layout caches (galleys) | Rendering text creates laid-out copies | Freed one frame after they stop being drawn; the lock screen draws no content. Freed memory is **not** zeroized. |
| egui `TextEdit` undo history | Snapshots of edited text | Lives in egui memory; never persisted (eframe `persistence` disabled); wiped on lock and when a tab closes. Freed, not zeroized. |
| Temporary `String`s inside serde_json and egui | Library internals | Bounded by document size; freed after use. |
| OS accessibility tree (AccessKit) | egui exposes widget labels and editor text to assistive technologies such as screen readers | Required for accessibility. Any process the user has granted OS accessibility permission can read what is on screen while unlocked; review that permission list. The tree is rebuilt without document content once locked (checked by the UI tests). |
| GPU textures / font atlas | Glyph rasterization (glyph shapes, not text order) | Not controllable. |
| OS clipboard, clipboard managers, IME buffers | Outside the process | Auto-clear (only if unchanged), privacy hints; documented. |
| Swap, hibernation images, crash reporters | OS-level | Core dumps disabled for the process; use full-disk encryption (FileVault, BitLocker, LUKS). |
| Plaintext exports and imported originals | User actions | Plaintext export needs a specific confirmation naming the destination; imports never delete originals and the UI says so. |

Zeroizing one buffer never guarantees that every copy in the process or OS
is gone. The canary tests (`crates/vault-core/tests/security.rs`) check
that distinctive fake secrets do not appear in files the app writes, logs,
diagnostics, recovery copies or exports — they cannot inspect process
memory or OS caches.

## Design choices driven by this model

- One standard age file per vault, so independent tools can recover data
  (A1, A5) and no custom cryptography is introduced.
- No plaintext metadata outside the encryption (A1, A6).
- Strict validation and resource limits before allocation (A3).
- Fingerprint conflict detection and conflict copies instead of
  last-writer-wins or automatic merging (A4).
- Auto-lock policy never writes plaintext, even at the cost of discarding
  edits when both the save and the encrypted recovery copy fail; the user
  is told on next unlock (A5, A6).
- No networking code, OS network denial on macOS (A9).

## Out of scope for v1

Hardware keys, biometrics, multi-user sharing, recovery keys/escrow,
plausible deniability, protection of file names/sizes, secure deletion,
automatic updates, and defending against a compromised OS. See
[ROADMAP.md](ROADMAP.md).
