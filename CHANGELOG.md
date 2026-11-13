# Changelog

Changes made in this build, newest first.

## Unreleased

Based on AddonForge v0.4.0 (build 31 source, commit `7bdd46b`), with
upstream's later changes merged through `73fdbd5` (13 November 2026).

### Added

- **From upstream: addons installed by the CurseForge app are recognised.**
  A folder with no catalogue entry, no id in its TOC and no source is matched
  by its CurseForge fingerprint (a hash of the files the addon loads), all
  unknown folders in one request per update check. A match gives it the
  project's id, so it groups with its modules and updates from CurseForge, or
  stays link only when the author opted out. In the fork this request goes
  through the network gate: nothing is hashed or sent offline or with
  CurseForge switched off. `addonforge --cli fingerprints` lists each folder's
  result.
- **From upstream: QuestieDB from CurseForge is checked as itself**, and
  Questie then updates from CurseForge too, so the GitHub bundle never
  overwrites a separate QuestieDB.
- **From upstream: 12 catalogue entries** (140 in all) and fresh download
  totals. The catalogue is upstream's byte for byte, so its signature still
  verifies.

- **Network settings and offline mode** (Settings, Network). Every request
  the app makes now goes through one gate in the core, which decides before
  anything is sent:
  - **Offline mode** stops every request. Scanning, pinning, removing,
    installing from a zip and WTF backups keep working, the catalogue is the
    copy built into the app, and a header badge says the app is offline.
  - **When AddonForge opens**: the update check, the catalogue refresh and the
    download totals can each be switched off.
  - **Addon sites**: CurseForge, GitHub, WoWInterface and Tukui can each be
    switched off. A site that is off is never contacted: searching or
    installing from it stops with a sentence saying so, its addons read "Not
    checked" instead of failing, and its images stay off too.
  - **Remote images** (avatars, catalogue icons, README pictures) can be
    switched off.
  - **Network activity**: what was sent or refused since the app opened, by
    purpose, host and path (no query strings), with counts. Kept in memory only.

  Everything is on by default, so a settings file from before behaves as it
  did. `addonforge --cli network [<switch> on|off]` does the same from the
  command line, and diagnostics name the settings.
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

- **`zip` 2.4 replaced by 9.5.** Four advisories against 2.4
  (RUSTSEC-2026-0367, -0368, -0369, -0429) concern `ZipArchive::extract`,
  symlink targets and Unix permissions. AddonForge never calls `extract` (it
  writes each entry itself, symlink entries as plain files, with no
  permissions), so none reached it; the vulnerable code is gone all the same.
  `tokio` moved past an unsoundness fix (RUSTSEC-2026-0386, Unix only) and a
  yanked crate was replaced.
- **CI checks the dependency tree and the history.** `cargo deny` (RustSec
  advisories, licences, crates.io only, no wildcard versions) and a gitleaks
  scan of every commit run on each pull request, on main and weekly; clippy
  runs with warnings as errors on Windows and macOS. Tools and actions are
  pinned by version and checksum or commit.
- **Redirects are held to each site's own hosts** (AUDIT O1). Every request
  names what it is for, and may only reach that purpose's hosts, on the first
  request and on every redirect: a GitHub download may move between GitHub's
  hosts, a WoWInterface download stays on WoWInterface's, an image stays on the
  image hosts. Before, only CurseForge's key-carrying requests were held;
  everything else could be redirected to any https host, and the hosts in
  Wago, WoWInterface, Tukui and GitHub replies were taken as given.

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

- A revoked CurseForge key in use, and the status the CDN returns for one.
  Search, install, update and an opted-out addon were run with the owner's
  real key on 13 November 2026, against a throwaway game folder. See
  `docs/PROVIDER_INTEGRATIONS.md`.
- The network switches in the running window were not clicked by hand. The
  window was checked by screenshot (offline badge, "Not checked" rows, the
  Network section), the handlers are covered by `scripts/test-ui.js` with
  Tauri's API stubbed, and the gate itself by Rust tests and the CLI.
- A live WoWInterface download through the gate: the catalogue prefers GitHub
  or CurseForge for every WoWInterface addon tried. Its host rule is tested
  against the real download address, which serves without a redirect.
- Install from zip in the running window: the file dialog and dropping a file
  were not driven by hand. The CLI path was run end to end against a throwaway
  game folder, the window was checked by screenshot, and the dialog and drop
  handlers are covered by `scripts/test-ui.js` with Tauri's API stubbed.
