# Roadmap

These items are **deliberately not implemented** in v1. No speculative
infrastructure exists for them; each needs its own design and review.

## Explicitly excluded from v1

| Feature | Why not now / what it would need |
|---|---|
| Accounts, hosted sync, sharing, collaboration | Contradicts the offline model; would need an authenticated protocol, server threat model and multi-key encryption. Use your own file sync. |
| Telemetry, crash uploads, update checks | Network features are out of scope. Diagnostics stay local and manual. |
| Automatic updates | Needs signed update metadata, rollback/freeze protection and an opt-in UI. Manual updates only. |
| Hardware keys, biometrics, OS keychain unlock | Would introduce device-bound secrets; must preserve "file + passphrase opens anywhere". Candidates: age plugin recipients (YubiKey) as an *additional* recipient. |
| Recovery keys / escrow | A second secret with its own storage problem; must be opt-in and documented. |
| AI assistants, browser extensions, autofill, TOTP | Large attack surface or network requirements. |
| Plugins, scripting, shell commands, terminal emulator | Arbitrary code execution inside a secrets app. The command palette lists app actions only. |
| Binary attachments, PDF/Office preview, OCR, unlimited sizes | Parser attack surface and memory limits; v1 is text only (2 MiB per document, 32 MiB per vault). |
| Mounted filesystems, background folder scanning, auto-ingestion | Would create plaintext outside the vault or read data the user didn't pick. |

## Likely next steps

- **OS-enforced network denial on Linux and Windows** (Landlock network
  rules / seccomp on Linux; AppContainer or WFP-based restrictions on Windows).
- **Screen-lock detection on Linux and Windows** (logind `Lock` signal via
  D-Bus; `WTSRegisterSessionNotification`).
- **Security-scoped bookmarks** if an App Sandbox build is ever offered.
- **Hide window from screen capture** where the OS supports it
  (`NSWindow.sharingType`, `SetWindowDisplayAffinity`).
- **Schema v2 migration framework** (explicit, tested migrations).
- **Independent security review**, then remediation — a prerequisite for
  any stronger assurance claim than "unaudited alpha".
- **Reproducible-build verification** by an independent second builder.
- **Per-document encrypted export of history** and an import of whole
  folders (still user-selected only).
- **Undo history held in zeroizing buffers** (custom undoer for egui's
  `TextEdit`) to remove a documented plaintext residual.
