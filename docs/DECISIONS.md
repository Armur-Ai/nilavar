# Design decisions

Concise records of material choices. Each lists the alternatives considered.

### D1. Product name and identity in one place
The working name in the original brief was NULLVAULT; the repository and
product are **Nilavar**. `vault-core/src/app.rs` holds the name, slug,
bundle identifier (a placeholder namespace — no domain is claimed),
extension and default filename, so renaming touches one module plus
packaging metadata.

### D2. Rust + egui/eframe + age
Native, single binary, no web view (so the Markdown preview cannot become a
browser). `age` passphrase mode is a documented, widely implemented format,
which makes independent recovery possible. No custom cryptography, no
layered ciphers, no custom KDF.

### D3. One self-contained vault file, JSON inside
Everything sensitive (including names, tags, timestamps and history) is in
one age file, so a vault is trivially portable via any file sync and
recoverable with `age -d`. JSON is boring and inspectable. Alternatives:
SQLite (an extra format and engine inside the encryption, harder to
recover by hand), per-document files (leaks counts and sizes per document,
complex conflict handling).

### D4. Strict schema
`deny_unknown_fields` everywhere and an exact `schema_version`: an older
build refuses a newer vault instead of silently dropping fields on the next
save.

### D5. Encode on the UI thread, encrypt on a worker
JSON encoding of ≤ 32 MiB takes milliseconds; scrypt takes ~0.3 s. The
worker queue holds at most one job, so the app never accumulates a backlog
of plaintext snapshots. Lock epochs make late results from before a lock
harmless.

### D6. Work factor 2^18, accept up to 2^20
Matches the reference `age` default and costs ~0.31 s per save/open on an
Apple M5 Pro. 2^20 (~1 GiB of memory) bounds what a malicious file can
demand. Files from `rage` on very fast machines may exceed 2^20 and are
rejected — documented trade-off.

### D7. Autosave: debounce 3 s, max 45 s, retry after 20 s
With ~0.3 s per save, this keeps the CPU idle while typing and bounds data
at risk. History coalesces autosaves within 10 minutes so history stays
meaningful. ⌘S always saves immediately and records a checkpoint.

### D8. Conflict handling = detect + conflict copy
Ciphertext fingerprint compared before writing and again just before the
rename. No automatic merge and no last-writer-wins. A local advisory lock
coordinates instances on one machine; nothing pretends to be a distributed
lock.

### D9. Recovery copies in the app data directory
Writing extra files next to a synced vault would multiply files in the
user's cloud folder. Copies stay encrypted, are bounded (5 + 5), and the UI
says plainly they are not an independent backup.

### D10. Auto-lock never writes plaintext
Order: encrypted save → encrypted unsaved-changes copy → discard and record
only a non-sensitive flag. Losing unsaved edits in a double failure is
preferable to an unencrypted emergency file.

### D11. OS network denial via `sandbox_init` (macOS), not the App Sandbox
The App Sandbox grants access only to the exact file a user picks, which
breaks same-directory atomic temp files, conflict copies and remembered
vault paths (absent security-scoped bookmarks). The built-in `no-network`
sandbox profile gives kernel-enforced network denial without changing file
semantics. The trade-off: no OS-enforced *file* confinement. Linux
(Landlock/seccomp) and Windows equivalents are roadmap items.

### D12. egui `TextEdit` as the editor foundation, behind a zeroizing adapter
Writing an editor from scratch would cost IME, accessibility and platform
keyboard conventions. Edits go through a `TextBuffer` adapter that zeroizes
old allocations on growth. egui's undo snapshots and layout caches are
documented residuals, wiped/evicted on lock.

### D13. Copy interception
egui emits copy/cut as output commands; the app routes them through the
clipboard guard (privacy hints + timed clear) instead of letting egui-winit
write them directly, and never publishes selections to the Linux PRIMARY
selection.

### D14. Fonts with PUA stripped
Inter maps hundreds of Private Use Area code points, which shadowed the
Phosphor icon font in egui's fallback chain. Removing only those cmap
entries (outlines untouched; OFL without Reserved Font Names) is simpler
and more robust than per-icon font families.

### D15. UI tests drive the real app headlessly
The desktop crate is a library plus a two-line binary so egui_kittest can
drive `NilavarApp` through AccessKit and render screenshots with wgpu —
real workflows (unlock, edit, save, lock, conflict, idle lock) are tested
end to end without a display server.

### D16. Unsigned artifacts by default
Packaging scripts produce ad-hoc-signed, clearly labelled `UNSIGNED`
artifacts with checksums and SBOMs. Developer ID signing/notarization is a
documented manual step; no private keys are requested or stored.
