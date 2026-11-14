# Recovery

Nilavar's only recovery model is: **the vault file + the passphrase.**
There is no account, no password reset, no recovery key, no device key and
no developer backdoor. If you lose the passphrase, the data is gone.

## Checklist

- [ ] Keep **at least two copies** of the vault file in different places
      (e.g. a cloud drive *and* an offline USB stick). Recovery copies in
      the app data directory are on the same disk and are **not** a backup.
- [ ] Store the passphrase somewhere safe and offline (paper in a safe,
      or a separate password manager).
- [ ] Make periodic encrypted backups: *Settings → Vault → Back up now…*
      or `nilavar-cli backup vault.nvault.age --out backup.nvault.age`.
- [ ] **Test** that a copy opens on another computer — or at least run
      `nilavar-cli verify backup.nvault.age` — before you need it.
- [ ] After a **passphrase change**, remember that older copies (backups,
      recovery copies, cloud version history, conflict copies) still open
      with the *old* passphrase. Delete them if the old passphrase may be
      compromised.

## Opening a vault on a new computer

1. Install Nilavar (see the README).
2. Copy the vault file to the computer (or open it from your synced folder).
3. *Open vault…* → choose the file → enter the passphrase.

Nothing else is needed. Nilavar keeps no machine-specific secret.

## When something goes wrong

| Situation | What to do |
|---|---|
| "The vault file changed on disk" | Another device or sync client wrote the file. Choose **Save conflict copy**: your version is written as `<name> (conflict <time>).nvault.age` next to the original, which is left untouched. Open both and merge by hand. |
| A save failed (disk full, permissions) | The previous vault file is unchanged and your edits are still open. Free space / fix permissions and press ⌘S, or *Back up now…* to another location. |
| A notice says an automatic lock saved an "unsaved-changes copy" | *Settings → Vault → Recovery copies* → **Restore as new…** on the `UNSAVED CHANGES` entry; compare with your vault. |
| The vault won't open: "failed authentication" / "ends early" | The file is damaged or incomplete (often a sync still in progress). Wait for the sync to finish, then try a recovery copy or backup (restore as new). |
| The vault won't open: "written by an incompatible version" | It was saved by a newer Nilavar. Install that version; the file was not modified. |
| Older copy needed | *Settings → Vault → Recovery copies* (last-known-good copies), your backups, or your cloud drive's version history. Each opens with the passphrase that was current when it was written. |

## Recovering without the Nilavar app

Every vault is a standard age file containing documented JSON
([FORMAT.md](FORMAT.md)), so any age implementation can recover it:

```bash
# Reference Go implementation (https://github.com/FiloSottile/age)
age -d -o payload.json vault.nvault.age        # prompts for the passphrase
# or the Rust implementation (https://github.com/str4d/rage)
rage -d -o payload.json vault.nvault.age
```

`payload.json` is now **plaintext** — handle it accordingly. Each entry in
`documents[]` has a `title` and `text`; `history[]` holds older versions and
`trash[]` holds deleted documents. For example, with `jq`:

```bash
jq -r '.documents[] | "## \(.title)\n\n\(.text)\n"' payload.json > all-documents.md
```

Standalone exports (*Export encrypted*) decrypt directly to the document's
text: `age -d note.txt.age`.

## The recovery CLI

`nilavar-cli` uses the same core library as the app — it is **not** an
independent implementation; the age/rage route above is.

```bash
nilavar-cli verify vault.nvault.age                       # decrypt, authenticate, validate; print counts
nilavar-cli list vault.nvault.age [--titles] [--trash]    # document ids and sizes
nilavar-cli export vault.nvault.age --doc <id> --out note.txt.age            # encrypted
nilavar-cli export vault.nvault.age --doc <id> --out note.md --plaintext      # typed confirmation
nilavar-cli recover vault.nvault.age --to-vault restored.nvault.age          # re-encrypt as a NEW vault
nilavar-cli recover vault.nvault.age --plaintext-dir ./recovered              # all docs as files (confirmed)
nilavar-cli recovery-copies vault.nvault.age              # where recovery copies are kept
```

Passphrases come from the terminal (no echo), or one per line from stdin
with `--passphrase-stdin`. Plaintext is never written to standard output;
plaintext output requires an explicit destination and either typing
`export plaintext` or `--confirm-plaintext-to <same path>`.
Exit codes: 0 ok, 2 error, 3 wrong passphrase / not a vault, 4 conflict or
vault in use, 5 not confirmed / cancelled.
