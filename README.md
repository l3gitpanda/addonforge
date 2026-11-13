# AddonForge

> **Private build.** A privacy-first build for personal use. Windows builds are published to [Releases](https://github.com/l3gitpanda/addonforge/releases) automatically from `main`, or build it from source (see [Build](#build)). It does not update itself: get new builds from Releases. What has changed is in [CHANGELOG.md](CHANGELOG.md); what is planned is in [docs/FORK_ROADMAP.md](docs/FORK_ROADMAP.md).

A small, fast addon manager for **World of Warcraft: Forever**.

**[Download the latest release](https://github.com/l3gitpanda/addonforge/releases/latest)** for Windows 10/11 (macOS from source). Two flavours, same app:

- `AddonForge-Setup-…exe`: installs to your user folder with a Start menu shortcut and an uninstaller. No admin rights needed.
- `AddonForge-v…exe`: portable, run it from anywhere, nothing installed.

Each release lists SHA-256 checksums in `SHA256SUMS.txt`.

The Windows files are not code-signed, so SmartScreen may show an unknown-publisher warning.

![AddonForge showing installed addons with update status](docs/screenshot.png)

- **Light.** A single exe on top of the WebView2 that Windows already has. No bundled browser.
- **Private.** No account, no ads, no analytics, no server of ours in the loop. The app talks only to the sites addons are published on (GitHub, Wago, WoWInterface, TukUI, CurseForge), plus this project's GitHub repo for the catalogue. The full list is under [Privacy](#code-signing-policy).
- **Honest.** If an addon's author only allows downloads through CurseForge's own app, AddonForge says so and gives you the link instead of pretending.

## How it finds addons

The app reads your `Interface\AddOns` folder and each addon's `.toc` file. Authors put their Wago / WoWInterface / CurseForge ids in there, and the BigWigs packager most authors use publishes a `release.json` with every GitHub release, so matching what you have to where it came from is mostly reading text files.

On top of that there is a **catalogue**: [`catalog/forever.json`](catalog/forever.json). It is a pointer list, not a host. Each entry says where the author publishes releases; downloads always come straight from there. Anyone can add an addon with a pull request.

## What it does

- **Starter packs** for people new to addons: six hand-picked sets (a good start, levelling, dungeons and raids, quality of life, bags, chat) with one-click install, plus a plain-English guide to what addons are, where they live and how to turn them on.
- Click any addon to read about it: description, download count, last update, the first screenshot and text from its README (fetched through GitHub, cached for a day).
- Sort the catalogue by name, most downloaded or recently updated. Download totals come from a daily GitHub Action that sums each author's release downloads into [`catalog/stats.json`](catalog/stats.json).
- Finds your WoW: Forever install and lists every addon in it, grouped properly (BigWigs and its twelve zone folders are one row).
- Checks for updates in parallel, straight from where each author publishes.
- Installs from the catalogue, from **any GitHub repo** you paste in, or from a **CurseForge search** covering every Forever addon they list (with your own CurseForge API key, added in Settings).
- Installs a **zip you downloaded yourself** (Install from zip… in My addons, or drop it on the window), with the same safety checks as a download. That is the way in for addons whose authors allow only CurseForge's own app. AddonForge keeps track of them but never updates them; it still tells you when CurseForge lists a newer version.
- Tells you when an addon is missing a required dependency, with a one-click install if the catalogue knows it.
- Pin an addon to hold it at its current version. Ignore one to hide it.
- Optional pre-release toggle for authors who only ship Forever builds as betas.
- Export your addon list to a small file, import it on another PC or send it to a friend.
- Refuses any zip containing a known executable or script type, or a file that starts like a Windows or macOS program or a script. Addons are Lua, XML and media, and AddonForge never runs anything from a zip.
- "Copy diagnostics" in Settings builds a report (version, install folder, addon list, recent log lines) and copies it, without sending anything.
- Keeps a small local log next to its settings file, never uploaded.

## Sources

| Source | Key needed? | Notes |
|---|---|---|
| GitHub releases | No | Preferred. Uses the packager's `release.json` when present. |
| Wago Addons | Link only | Wago gives updater keys to its Patreon supporters only, so AddonForge no longer installs from Wago. Entries that live only there link to their Wago page. |
| WoWInterface | No | Public JSON API. |
| TukUI | No | ElvUI / Tukui. |
| CurseForge | Your own | Search all Forever addons on CurseForge and install the ones whose authors allow third-party downloads (about seven in ten of the popular ones). Needs a CurseForge API key of your own, added in Settings: CurseForge issues each key to one person and does not allow it to be passed on, so this fork does not ship one. The key is tested once, then kept in Windows Credential Manager (macOS: the Keychain), never in the settings file, log, diagnostics or an exported list, and is sent only to CurseForge: api.curseforge.com, and its download servers (forgecdn.net) when a file is fetched. API responses aren't cached on disk; the only CurseForge data AddonForge keeps is the project id and version name of what you installed, in its settings and log. Opted-out addons are link only: download the zip on their page and use Install from zip. Details: [docs/PROVIDER_INTEGRATIONS.md](docs/PROVIDER_INTEGRATIONS.md). |
| Zip file | No | A zip you downloaded yourself. Installed through the same checks as a download, kept track of, never updated. Nothing is fetched. |

## Build

Needs Rust (rustup), the MSVC build tools and Node.

```
npm install
npm run dev      # run with hot reload of the UI folder
npm run build    # portable exe in src-tauri/target/release/
```

The original ember-and-anvil app icon lives in `icon.svg`. Run
`powershell -File scripts/make-icons.ps1` after editing it to refresh the header
and packaged desktop icons, then rebuild the app. The Windows executable and
installer both embed `src-tauri/icons/icon.ico`.

## macOS

AddonForge also runs on macOS, built from source (no Mac release yet).
WoW on macOS lives in `/Applications/World of Warcraft`; the app looks there and
in `~/Applications`. Anywhere else (e.g. an external drive): pick the folder in Settings.
Install the Xcode Command Line Tools (`xcode-select --install`), Rust and Node, then:

```
npm install
npx tauri build --bundles app   # src-tauri/target/release/bundle/macos/AddonForge.app
```

The app isn't signed, so the first launch needs right-click → *Open*. Settings
and the log live in `~/Library/Application Support/AddonForge`.

## Adding addons to the catalogue

See [CONTRIBUTING.md](CONTRIBUTING.md): one JSON entry in `catalog/forever.json`, validated by `scripts/validate-catalog.js`.

## Privacy

AddonForge does not collect or send personal data and has no telemetry, accounts or analytics. It contacts only these hosts, and only to check versions, read addon details and download addons:

- github.com, api.github.com, raw.githubusercontent.com and GitHub's release download hosts: the addon catalogue and GitHub-hosted addons (addon details, README images and author avatars).
- api.curseforge.com, edge.forgecdn.net and the forgecdn.net host it hands downloads to: CurseForge addon lookups and downloads (only once you have added your own API key, and only for addons whose authors allow third-party downloads).
- api.mmoui.com and cdn.wowinterface.com: WoWInterface addon lookups and downloads.
- api.tukui.org: TukUI addon lookups and downloads.
- media.forgecdn.net, wago.io hosts, www.wowinterface.com and cdn-wow.mmoui.com: only for the preview image, when the first screenshot in a GitHub-hosted addon's README lives there.

Every request is HTTPS and goes through one gate in the app's core: each kind of request may only reach its own site's hosts, on the first request and on every redirect (GitHub's downloads redirect between GitHub's own hosts). At launch the app reads the catalogue and download totals from this repo, then checks your installed addons for updates. **Settings, Network** turns each of those off, switches off any addon site or remote images, or turns on offline mode, which stops every request; it also lists what was sent or refused since the app opened. The app never looks for updates to itself.

These requests reveal your IP address, your AddonForge version (in the User-Agent) and the addon names or IDs being checked, as with any web request. For addons it can't otherwise identify, an update check also sends CurseForge their folder fingerprints: numbers hashed from the addon files, never the files themselves, and only when you have a CurseForge key and CurseForge is switched on. The answer is kept in memory only. Your CurseForge API key is sent only to CurseForge: to api.curseforge.com, and with a file download to its forgecdn.net servers, which have required it since July 2026. A request carrying the key is never allowed to be redirected anywhere else.

MIT licensed, see [LICENSE](LICENSE).

AddonForge is an independent project. It is not affiliated with, endorsed by or connected to CurseForge, Overwolf, Blizzard Entertainment, Wago, WoWInterface or TukUI. World of Warcraft is a trademark of Blizzard Entertainment.
