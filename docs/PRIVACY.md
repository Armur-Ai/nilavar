# Privacy

Nilavar has **no accounts, servers, telemetry, analytics, crash uploads,
update checks or networking code**. Nothing leaves your computer unless you
copy a file somewhere yourself, copy text to the clipboard, or explicitly
open a link from the Markdown preview in your browser.

## What is stored, where

| Data | Stored | Encrypted? |
|---|---|---|
| Documents, titles, folders, tags, history, trash, vault preferences | Inside the vault file you chose | **Yes** (age) |
| Last-known-good and unsaved-changes recovery copies | App data dir → `recovery/` | **Yes** (age, with the passphrase current at the time) |
| UI settings: effects, reduce motion, font size, soft wrap, line numbers, interface scale, lock-on-focus-loss | App data dir → `settings.json` | No (contains no content) |
| Path of the last opened vault | `settings.json`, only while *Remember the last vault's location* is enabled (default **on**; turn it off in *Settings → Security*) | No |
| "An automatic lock could not save" notice | `settings.json`: a flag and an error code such as `NV-305`, nothing else | No |
| Local lock file | App data dir → `locks/` (empty file named by a hash of the vault path) | n/a |
| Diagnostic report | Only where *you* save it; contains versions, capability flags and error codes | No (contains no content) |
| Plaintext exports | Only where you choose, after an explicit confirmation | **No** — by definition |

App data locations: macOS `~/Library/Application Support/Nilavar`, Linux
`~/.local/share/nilavar`, Windows `%APPDATA%\Nilavar` (debug builds:
`…-dev`). Deleting the app data directory removes settings and recovery
copies; it does not touch your vaults.

## What others can observe

Anyone who can see your files (cloud provider, someone with your disk)
can see a vault's **file name, size, and creation/modification times**,
how often it changes, and the existence/timestamps of recovery copies in
the app data directory. They cannot see titles, folder names, tags or
contents. The neutral default name `vault.nvault.age` avoids describing the
contents; the `.nvault.age` extension does reveal that it is a Nilavar
vault — rename it freely if that matters to you.

## Clipboard

Copying is deliberate and best effort. Nilavar asks the OS to keep copied
text out of clipboard history where supported and clears it after a delay
(default 30 s) and on lock — but only if the clipboard still holds what
Nilavar put there. Other apps and clipboard managers may already have read
it.

## Screenshots and screen sharing

Nilavar does not prevent screen capture or recording. Lock the vault
before sharing your screen.

## Development builds

Debug builds keep their data in a separate `…-dev` directory so testing
never touches your real settings. Project tests and screenshots use only
synthetic data.
