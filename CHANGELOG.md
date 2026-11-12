# Changelog

Changes made in this build, newest first.

## Unreleased

Based on AddonForge v0.4.0 (build 31 source, commit `7bdd46b`).

### Added

- **Install from a zip file.** For addons AddonForge is not allowed to
  download, such as the many CurseForge projects whose authors allow only
  CurseForge's own app (NetherSuite, Details!, DBM, Bagnon, Leatrix Plus,
  Auctionator...): download the zip in your browser, then use **Install from
  zip…** in My addons, or drop the zip on the window. AddonForge copies the zip
  to its downloads folder and installs the copy through exactly the path a
  download takes: the install lock, extraction limits, the safety scan, name
  checks, rollback and recovery. Your own file is never changed or removed.
  The addon then appears in My addons as installed by AddonForge (pin it,
  remove it) with the status "From a zip file", and AddonForge never claims it
  can update it: Update is not offered, and a newer zip installs over it the
  same way. Installing the same addon later from GitHub or CurseForge takes
  over the zip install (keeping its pin) when the download holds all of its
  folders and none of another addon's. If its TOC carries a CurseForge project id, the update check still
  says when CurseForge lists a newer version ("X is on CurseForge; you have
  Y"). A zip with no Forever TOC asks first, as a download does. Nothing is
  downloaded and no CurseForge page is read; the only request is the
  catalogue, to name the addon.
- `addonforge --cli install-zip <file.zip> [--force]`.
- **Your own CurseForge API key.** CurseForge search and install were switched
  off in any build without upstream's built-in key, which a fork does not have.
  A key can now be added in Settings (or with `addonforge --cli curse-key set`,
  read from standard input). It is tested against CurseForge once, then kept in
  the operating system's credential store (Windows Credential Manager, macOS
  Keychain). It is never written to the settings file, the log, diagnostics or
  an exported list, and the interface is never given the key back.
- `addonforge --cli curse-key status | set | clear`.
- Find addons says when CurseForge is off and links to the setting. An
  installed addon that lives only on CurseForge says that a key would let
  AddonForge update it, when that is true.

### Changed

- **Link-only rows say why.** A row AddonForge can't install used to read
  "link only", with the reason only in a tooltip, so an addon whose author has
  turned off downloads in other apps looked like a key that didn't work. The
  row now names the reason: "CurseForge app only" (the author's choice; a key
  can't change it), "needs API key", "no Forever file" or "Wago only", with the
  full sentence on hover and in the details pane. Installing such an addon, or
  an installed one's status line, says the same, and the internal
  `CURSE_OPTOUT` tag no longer reaches the message. These rows, and an
  installed row with a newer release on CurseForge, now point at Install from
  zip.
- **A fork build no longer updates itself from the original project.** The
  self-updater still pointed at the original's releases, and the app holds the
  original's signing key (for the catalogue), so a fork build offered "Update
  now" and would have replaced itself with the original app. Self-update now
  needs an update channel of the build's own (`selfupdate::UPDATE_CHANNEL`),
  and the fork has none yet: no update manifest is requested at launch, nothing
  is offered, and nothing is installed, including when `ADDONFORGE_UPDATE_URL`
  or `--cli selfupdate <url> --force` points at a manifest. Settings says so in
  place of the "check now" link. Addon updates are unaffected.
- **CurseForge downloads now carry the API key**, as CurseForge has required
  for its file servers since 16 July 2026. Before, the key went only to
  `api.curseforge.com` and downloads went without it. The key is attached only
  to an `https` link on `forgecdn.net`.
- The redirect rule that keeps the key on CurseForge's hosts now also covers
  downloads: a download may move between `forgecdn.net` hosts and nowhere else.
- A failed CurseForge download is explained without its download link, so the
  link no longer reaches the log.
- A build with no built-in key is the default. Baking one in is still possible
  for a maintainer whose agreement with CurseForge allows it.
- Diagnostics report whose CurseForge key is in use (your own, built in, none),
  never the key.

### Security

From the Phase One audit; details and the open items in `docs/AUDIT.md`.

- **Archive names Windows would store differently are refused.** A zip entry
  such as `notes.lua:payload.exe` became a hidden alternate data stream that
  the executable check never saw. Device names (`CON`, `NUL`, `COM1`...),
  names ending in a dot or space, and characters Windows does not allow are
  refused too.
- An archive whose folders resolve to an empty name, or two folders to the
  same name, is refused before anything in AddOns moves.
- Every reply from a server is read with a size limit (16 MB for API replies,
  2 MB for READMEs, 4 MB for the catalogue). Before, most were read whole.
- Remote images are decoded with a limit on their dimensions, so a small file
  cannot claim gigabytes of pixels.
- A download that fails part-way no longer leaves a partial zip behind, and
  zips abandoned for more than 6 hours are removed at startup.
- Moving a folder to the Recycle Bin works when WoW is on a network share; the
  path handed to Windows was relative before.
- Recovery after an interrupted install logs restored folders correctly.

### Dependencies

- Added `keyring-core` 1, with `windows-native-keyring-store` 1 on Windows and
  `apple-native-keyring-store` 1 on macOS (MIT or Apache-2.0).

### Not verified

- A search, install and update with a real CurseForge key. The fork has no key
  yet. See `docs/PROVIDER_INTEGRATIONS.md`.
- Install from zip in the running window: the file dialog and dropping a file
  were not driven by hand. The CLI path was run end to end against a throwaway
  game folder, the window was checked by screenshot, and the dialog and drop
  handlers are covered by `scripts/test-ui.js` with Tauri's API stubbed.
