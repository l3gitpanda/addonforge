# Changelog

Changes made in this build, newest first.

## Unreleased

Based on AddonForge v0.4.0 (build 31 source, commit `7bdd46b`).

### Added

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
