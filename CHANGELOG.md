# Changelog

Changes made by this fork, newest first. The original project's history is in
its own releases: <https://github.com/Topher121/AddonForge/releases>.

## Unreleased

Forked from Topher121/AddonForge at v0.4.0 (build 31 source, commit `7bdd46b`).

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

### Dependencies

- Added `keyring-core` 1, with `windows-native-keyring-store` 1 on Windows and
  `apple-native-keyring-store` 1 on macOS (MIT or Apache-2.0).

### Not verified

- A search, install and update with a real CurseForge key. The fork has no key
  yet. See `docs/PROVIDER_INTEGRATIONS.md`.
