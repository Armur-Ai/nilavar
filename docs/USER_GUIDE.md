# Nilavar — User Guide

*No technical knowledge needed. About 10 minutes to read.*

Nilavar is a private notebook for the things you can't afford to lose or
leak: passwords, recovery codes, bank details, Wi-Fi keys, private notes.
Everything you write is locked inside **one encrypted file** that only your
passphrase can open.

---

## 1. The big idea (read this first)

- **Your vault is a single file** (it ends in `.nvault.age`). All your notes
  live inside it, scrambled so nobody can read them without your passphrase.
- **Nilavar never connects to the internet.** There is no account, no
  login, no cloud, no company server. Nothing is uploaded — ever.
- **That means no automatic sync between devices.** If you want your notes
  on another computer, you move the vault *file* there yourself (USB stick,
  Google Drive, Dropbox, email to yourself — anything that moves files).
  Because the file is encrypted, it's safe to put it in those places.
- **Your passphrase is the only key.** There is no "forgot password" button
  and nobody — not even the people who made Nilavar — can open your vault
  without it. **Write it down and keep it somewhere safe.**

---

## 2. Install Nilavar

Go to the **[Download page](https://github.com/Armur-Ai/nilavar/releases/latest)**
and pick the file for your computer.

### Mac
1. Download **`Nilavar-macOS-universal.dmg`** and double-click it.
2. Drag **Nilavar** into the **Applications** folder.
3. The first time only: open **Applications**, **right-click** Nilavar →
   **Open** → **Open**. (macOS warns because this free app isn't registered
   with Apple. If there's no Open button, go to **System Settings → Privacy &
   Security** and click **Open Anyway**.)

### Windows
1. Download **`Nilavar-Windows-x64.exe`**.
2. Double-click it. If Windows shows **"Windows protected your PC"**, click
   **More info → Run anyway** (this appears because the app isn't
   code-signed).
3. Tip: move the `.exe` to a folder like *Documents\Nilavar* and pin it to
   your taskbar.

### Kali Linux, Debian, Ubuntu, Linux Mint
Download **`nilavar_amd64.deb`**, then double-click it, or in a terminal:
```
sudo apt install ./nilavar_amd64.deb
```
Then open **Nilavar** from your applications menu.

### Arch Linux, BlackArch, Manjaro, EndeavourOS
Download **`nilavar-x86_64.pkg.tar.zst`**, then:
```
sudo pacman -U nilavar-x86_64.pkg.tar.zst
```

### Any other Linux
Download **`nilavar-linux-x86_64.tar.gz`**, extract it, and run
`./install.sh` inside the folder.

---

## 3. Create your vault (first time)

1. Open Nilavar and click **Create new vault…**
2. Click **Choose location…** and pick where to save your vault file.
   - Good choices: your **Documents** folder, or a **Google Drive /
     Dropbox / OneDrive** folder if you want an automatic off-site copy.
   - The suggested name `vault.nvault.age` is deliberately boring — anyone
     who sees the file can't tell what's in it.
3. **Choose a passphrase.** Easiest and strongest: click **Suggest a
   passphrase** — you'll get seven random words like
   `orbit-cactus-lantern-…`. The meter shows how strong it is.
4. **Write the passphrase on paper** and keep it somewhere safe (a drawer at
   home, a safe). If you forget it, your vault cannot be opened. Ever.
5. Tick **"I understand there is no password reset…"** and click
   **Create encrypted vault**.

You're in. There's a short *Welcome* note to get you started.

---

## 4. Everyday use

| To… | Do this |
|---|---|
| Write a new note | **⌘N** (Mac) / **Ctrl+N** (Windows/Linux), or the 📄+ button in the sidebar |
| Find a note | Type in **Search vault** at the top-left, or press **⌘P / Ctrl+P** |
| Organize | Right-click in the sidebar → **New folder**, **Move to**, **Rename**; add **tags** in the ⓘ Info pane |
| See formatted text | Click the 👁 eye button (or **⌘E / Ctrl+E**) |
| Undo a mistake | **⌘Z / Ctrl+Z**, or open 🕘 **History** and **Restore** an older version |
| Copy a password | Select it and copy — Nilavar **clears it from the clipboard** after 30 seconds |
| Delete a note | Right-click → **Move to trash** (you can restore it from **Trash**) |
| Lock now | Click **Lock** (top-right) or **⇧⌘L / Ctrl+Shift+L** |

**Saving is automatic.** The little light in the bottom-left corner shows
the real state: **SAVED** (green), **UNSAVED** (amber), **SAVING…**, or
**SAVE FAILED** (red — click *retry*). You can always press **⌘S / Ctrl+S**.

**Auto-lock.** Nilavar locks itself after 5 minutes without use, when your
computer sleeps, and (on Mac) when your screen locks. Change this in
**Settings → Security**.

Notes can use simple formatting (Markdown): `# Heading`, `**bold**`,
`- list`, `- [ ] checkbox`. Or switch a note to plain text from the status
bar.

---

## 5. Using your vault on another computer

Nilavar doesn't sync by itself — **you carry the file**. It's encrypted, so
carrying it is safe.

```
 Laptop                                   Desktop / new computer
 ┌────────────────────┐   copy the file   ┌────────────────────┐
 │ vault.nvault.age   │ ────────────────▶ │ vault.nvault.age   │
 │ (encrypted)        │  USB · Drive ·    │ open with the SAME │
 └────────────────────┘  Dropbox · email  │ passphrase         │
                                          └────────────────────┘
```

1. Copy `vault.nvault.age` to the other computer (USB stick, cloud folder,
   email it to yourself, AirDrop…).
2. Install Nilavar there.
3. Click **Open vault…**, choose the file, type **the same passphrase**.

That's all. No account, no setup, no "device approval".

**Using a cloud folder (Google Drive, Dropbox, OneDrive, iCloud)?** Save
the vault inside it and it travels automatically — just let the sync finish
before opening it on the other computer, and **don't edit the same vault on
two computers at the same time**. If you do, Nilavar notices that the file
changed and **won't overwrite it** — it offers to save **your version as a
separate "conflict copy"** so nothing is lost. Open both and copy over what
you need.

---

## 6. Backups — please do this

Your vault is one file, so backing it up is easy:

- Keep **at least two copies in different places** — e.g. your cloud folder
  **and** a USB stick in a drawer.
- **Settings → Vault → Back up now…** writes an encrypted copy wherever you
  choose (it opens with the same passphrase).
- Every so often, **test** that a backup opens.

Nilavar also keeps a few automatic **recovery copies** on your computer
(**Settings → Vault → Recovery copies**). They help after accidents, but
they live on the same computer, so they are **not** a real backup.

---

## 7. Changing your passphrase

**Settings → Vault → Change passphrase…** Important: **old backups and old
copies still open with the old passphrase.** If you're changing it because
someone may know the old one, delete those old copies too.

---

## 8. Moving notes in and out

- **Import:** command palette (**⇧⌘P / Ctrl+Shift+P**) → *Import text
  files…*. Your original files are **not** deleted — delete them yourself
  if they contain secrets.
- **Share one note securely:** right-click → **Export encrypted (.age)…**,
  choose a separate password for it. The receiver can open it with Nilavar
  (*Import encrypted*) or the free `age` tool.
- **Export as plain text:** right-click → **Export as plaintext…** — this
  writes an **unprotected** file; Nilavar asks you to confirm and shows
  exactly where it will go.

---

## 9. Questions

**What if I forget my passphrase?**
The vault can't be opened. There is no reset, no recovery email, no
backdoor. That's what keeps it safe — so write the passphrase down.

**Can Nilavar's makers read my notes?**
No. Nothing ever leaves your computer, and the vault can only be decrypted
with your passphrase.

**Does it need internet?**
No. It works fully offline, forever. (On Mac it even blocks its own
network access at the system level.)

**Is it safe to keep my vault in Google Drive or Dropbox?**
Yes, the file is encrypted — they only see a file of a certain size. Use a
strong passphrase, since anyone with the file could try to guess it.

**What can someone see without my passphrase?**
Only that a file exists, its name, its size and when it changed. Not your
note titles, folders, tags or contents.

**The app says the vault "changed on disk".**
Another computer or your cloud app updated the file. Choose **Save conflict
copy** — nothing is overwritten. See section 5.

**Can I open my vault without Nilavar?**
Yes. It's a standard `age`-encrypted file. Someone technical can run
`age -d vault.nvault.age` with your passphrase to get your notes out as text
(see [RECOVERY.md](RECOVERY.md)). Your data never depends on this app
existing.

**What doesn't it protect against?**
A virus already on your computer, someone using your computer while the
vault is unlocked, or someone watching your screen. Lock it when you step
away. Nilavar is a young (alpha) project that hasn't had an independent
security audit yet — keep backups.
