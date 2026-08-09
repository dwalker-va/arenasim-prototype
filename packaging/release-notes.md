Download the file for your machine, then follow the one-time step below to open
it. You do not need Rust, a compiler, or anything else installed.

- **macOS (Apple Silicon)** — `ArenaSim-<version>-macos-arm64.dmg`
- **Windows (64-bit)** — `ArenaSim-<version>-windows-x64.zip`

Intel Macs are not supported. If `About This Mac` says Intel rather than Apple
M-something, this build will not run.

---

## Opening it the first time

These builds are not code-signed — signing costs a yearly Apple developer
membership and a Windows certificate, which is a lot for a hobby project shared
with friends. Your OS will therefore warn you the first time you open it. This
is the warning every unsigned app gets; it is not a virus report. Both systems
remember your answer, so you only do this once.

### macOS

1. Open the `.dmg` and drag **ArenaSim** into your **Applications** folder.
2. Open Applications and double-click **ArenaSim**.
3. macOS refuses, saying the app *"is damaged"* or *"cannot be opened because
   Apple could not verify it"*. Click **Done** — do not click *Move to Trash*.
4. Open **System Settings → Privacy & Security**, scroll to the bottom, and
   next to the line about ArenaSim being blocked, click **Open Anyway**.
5. Confirm with **Open Anyway** and unlock with Touch ID or your password.

The app opens, and every launch after this one goes straight through.

If step 4 shows nothing, the older shortcut still works: right-click ArenaSim
in Applications, choose **Open**, then **Open** again in the dialog.

Still stuck? In Terminal, run this and try again:

```
xattr -dr com.apple.quarantine /Applications/ArenaSim.app
```

### Windows

1. Unzip the file anywhere — your Downloads folder is fine. Keep `arenasim.exe`
   and the `assets` folder together; the game reads its data from `assets`.
2. Double-click **arenasim.exe**.
3. A blue **"Windows protected your PC"** box appears. Click **More info**, then
   **Run anyway**.

There is no installer, so uninstalling is deleting the folder.

---

## Where the game keeps your settings

Your options and keybindings persist between launches, and a report is written
for every match you watch.

- **macOS** — `~/Library/Application Support/ArenaSim/`
- **Windows** — `%APPDATA%\ArenaSim\data\`

Deleting that folder resets the game to defaults.

---

## Licensing

The code is MIT-licensed. The ability, class, and item icons are World of
Warcraft artwork from Wowhead and are not covered by that licence — see
`LICENSE` for the full picture.
