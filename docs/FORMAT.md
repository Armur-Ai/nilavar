# Vault file format (v1)

## Outer file

A vault is a standard **age v1** file (<https://c2sp.org/age>) encrypted to
a single **scrypt passphrase recipient**:

```
age-encryption.org/v1
-> scrypt <base64 salt> 18
<base64 wrapped file key>
--- <base64 header MAC>
<16-byte nonce><STREAM-encrypted payload: 64 KiB chunks, ChaCha20-Poly1305>
```

- Suggested extension: `.nvault.age`. Default name: `vault.nvault.age`.
- Written by the Rust `age` crate **0.12.1** (labelled beta by its
  maintainers). Work factor 18 when writing; files requiring more than
  **20** are rejected. Reference `age` (Go) also writes 18; `rage` picks a
  work factor targeting about one second and may exceed 20 on very fast
  machines — such files are rejected by design.
- Any age implementation can decrypt a vault: `age -d vault.nvault.age`.

## Payload

UTF-8 JSON, compact, field order as listed. Unknown fields anywhere are
rejected (so an older build can never silently drop data written by a newer
one). All ids are 32 lowercase hex characters (128 random bits).
Timestamps are Unix seconds (UTC).

```jsonc
{
  "format": "nilavar-vault",          // magic
  "schema_version": 1,                // anything else → "unsupported schema", file untouched
  "vault_id": "hex32",                // stable for the vault's lifetime (restore-as-new gets a new one)
  "revision_id": "hex32",             // new on every save
  "parent_revision_id": "hex32|null", // revision this save was derived from
  "save_counter": 12,                 // monotonic within one lineage
  "created_at": 0,
  "saved_at": 0,
  "prefs": {
    "idle_lock_secs": 300,            // 60..=86400
    "clipboard_clear_secs": 30,       // 5..=600
    "autosave": true,
    "lock_on_sleep": true
  },
  "folders": [
    { "id": "hex32", "name": "…", "parent": "hex32|null", "created_at": 0 }
  ],
  "documents": [ /* Document */ ],
  "trash": [
    { "deleted_at": 0, "document": { /* Document */ } }
  ]
}
```

Document:

```jsonc
{
  "id": "hex32",
  "title": "…",                       // may be empty ("Untitled" in the UI)
  "text": "…",                        // UTF-8, ≤ 2 MiB
  "folder": "hex32|null",
  "tags": ["…"],                      // unique, no whitespace/commas/control chars
  "mode": "markdown" | "plain",
  "pinned": false,
  "created_at": 0,
  "updated_at": 0,
  "history": [                        // oldest first; newest mirrors the last saved state
    { "id": "hex32", "saved_at": 0, "kind": "auto" | "checkpoint" | "restore",
      "title": "…", "text": "…" }
  ]
}
```

### History semantics

- History is recorded when saving, not per keystroke. A document whose
  title/text differ from its newest entry gets a new entry.
- Consecutive `auto` entries less than 10 minutes apart are merged.
  `checkpoint` (explicit save) and `restore` entries are never merged.
- At most 40 entries per document; the oldest is dropped beyond that (a
  documented retention policy). History is **never** pruned to make a save
  fit the size limit — the save is refused instead.

## Validation (on every open and before every save)

| Rule | Error |
|---|---|
| Encrypted input ≤ 64 MiB | NV-109 |
| scrypt work factor ≤ 20, passphrase recipient only | NV-108, NV-106 |
| Entire stream authenticated (header MAC, every chunk, final flag) | NV-103, NV-104, NV-105 |
| Decrypted payload ≤ 32 MiB | NV-110 |
| Valid UTF-8, JSON nesting ≤ 16 (checked before parsing) | NV-201, NV-203 |
| `format` and `schema_version` | NV-201, NV-202 |
| No unknown fields; correct types | NV-201 |
| Well-formed, unique ids (documents across live+trash, folders, history) | NV-205, NV-204 |
| Folder parents exist, no cycles, depth ≤ 12 | NV-206, NV-207, NV-208 |
| Document/trash folder references exist | NV-206 |
| Text ≤ 2 MiB (documents and history), names ≤ 256 B, tags ≤ 64 B and ≤ 32 per document | NV-209, NV-210, NV-211 |
| ≤ 5,000 documents, ≤ 1,000 folders, ≤ 5,000 trashed, ≤ 40 history per document, ≤ 20,000 history total | NV-211 |
| Preferences within their ranges | NV-201 |

A rejected file is never modified.

## Standalone document exports

*Export encrypted* writes a separate age passphrase file whose plaintext is
**only the document's UTF-8 text** (no JSON). Decrypt with
`age -d note.txt.age`, `rage -d`, or Nilavar's *Import encrypted*.

## Files outside the vault

| File | Location | Contents |
|---|---|---|
| `settings.json` | app data dir | UI preferences, effects, font size, last vault path (if enabled), a pending-notice flag with an error code. No content. |
| `recovery/<key>/lkg-<UTC timestamp>-<rand>.nvault.age` | app data dir | Encrypted copy of the vault file as it was before a save (≤ 1 per 15 min, newest 5 kept). `<key>` is a hash of the vault id. |
| `recovery/<key>/unsaved-<UTC timestamp>-<rand>.nvault.age` | app data dir | Encrypted session state written when an automatic lock could not save (newest 5 kept). |
| `locks/<hash of path>.lock` | app data dir | Empty advisory-lock file. |
| `.<vault name>.<random>.tmp` | next to the vault | Encrypted temporary file during a save; removed on failure, renamed on success, stale ones cleaned on open. |

App data dir: macOS `~/Library/Application Support/Nilavar`, Linux
`$XDG_DATA_HOME/nilavar` (or `~/.local/share/nilavar`), Windows
`%APPDATA%\Nilavar`. Debug builds use a separate `…-dev` directory;
`NILAVAR_DATA_DIR` overrides the location.

## Versioning

`schema_version` increases for any change that older builds could not read
faithfully. A newer build must migrate older schemas explicitly and must
never write a schema version older builds would partially understand.
