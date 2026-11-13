# Provider integrations

Where addons come from, what each source allows, and how the code treats it.
Status labels: Not started, In progress, Implemented, Tested, Blocked, Deferred.

| Provider | Code | Status | Access |
|---|---|---|---|
| GitHub releases | `src-tauri/src/sources/github.rs` | Implemented (upstream; not re-audited by the fork yet) | Public, no key. 60 API requests an hour without a token when pre-releases are on. |
| CurseForge | `src-tauri/src/sources/curseforge.rs` | **Tested live** with the owner's key, 13 November 2026 (see below) | API key required. The user's own, or one built in. |
| WoWInterface | `src-tauri/src/sources/wowi.rs` | Implemented (upstream; not re-audited) | Public JSON API, no key. |
| TukUI | `src-tauri/src/sources/tukui.rs` | Implemented (upstream; not re-audited) | Public, no key. |
| Wago | `src-tauri/src/sources/wago.rs` | Link only | Upstream dropped it in build 25: Wago's updater keys are for its Patreon supporters. Not re-investigated by the fork yet. |
| Local zip file | `install.rs` (`stage_local`, `place_planned`), `lib.rs` (`install_local`) | Tested (see [below](#local-zip-file)) | A file the user downloaded themselves. No network. |
| GitLab, Codeberg, any HTTPS archive | none | Not started | |

## CurseForge

### What the fork changed, and why

Upstream bakes one CurseForge API key into its release builds (`build.rs`, read
from a gitignored `private/curseforge-key.txt`). That key is not in the
repository, so a build of this fork has none, and with no key every CurseForge
function is switched off. That is why CurseForge search and install did not
work in the fork.

The fork cannot reuse upstream's key and should not ship one of its own by
default: CurseForge's terms issue a key to one developer and forbid disclosing
it (section 2.2, below), and a key inside a published exe can be read back by
anyone who has the file.

So the fork adds a **user-supplied key**:

- Entered in Settings (or `addonforge --cli curse-key set`, which reads it from
  standard input so it stays out of shell history).
- Checked for shape, then tested with one request (`GET /v1/games/1`). It is
  stored only if CurseForge accepts it for World of Warcraft.
- Stored in the operating system's credential store: Windows Credential
  Manager (machine-local persistence) or the macOS Keychain, through
  `keyring-core` and the platform store crates. See `src-tauri/src/secrets.rs`.
- Never written to `state.json`, the log, a diagnostics report or an exported
  addon list. The UI never receives the key back; it is told only whose key is
  in use (`user`, `embedded` or `none`).
- The user's own key wins over a built-in one. The build-time path still
  exists for a maintainer whose agreement with CurseForge covers shipping a key.

### Where the key is sent

| Host | When | Why |
|---|---|---|
| `api.curseforge.com` | Search, update check, resolving a file, fingerprint matching (`POST /v1/fingerprints/1`, merged from upstream) | The API. Header `x-api-key`. |
| `edge.forgecdn.net`, then the `forgecdn.net` host it redirects to (seen: `mediafilez.forgecdn.net`) | Downloading a file from a CurseForge project | CurseForge announced on 10 June 2026 that CDN downloads need the key from 16 July 2026, sent as `x-api-key`. |

Nowhere else. Two rules in code enforce it, inside the network gate
(`net.rs`), which also holds every CurseForge request to `api.curseforge.com`
and `forgecdn.net` and lets the user switch CurseForge off:

- `download_headers` attaches the key only to an `https` link whose host is
  `forgecdn.net` or a subdomain of it, and only for a CurseForge install.
- `redirect_keeps_key_home`, used by the HTTP client's redirect policy, refuses
  any redirect that would take a request from the API host to another host, or
  from the CDN to anything outside `forgecdn.net`. (reqwest drops
  `Authorization` on a cross-host redirect but keeps custom headers such as
  `x-api-key`, so without this a redirect could carry the key away.)

Upstream sent the key to `api.curseforge.com` only and downloaded files without
it. Measured on 12 November 2026 without a key: `edge.forgecdn.net` answers 302
to `mediafilez.forgecdn.net`, which answers **403**. So a download that does not
carry the key is refused today.

### Rules kept because CurseForge's terms ask for them

From the *CurseForge 3rd Party API Terms and Conditions* (Overwolf), as read on
12 November 2026. This is a reading by the project, not legal advice.

| Term | What it says | What the app does |
|---|---|---|
| 2.2 | A key is unique to its developer, non-transferable, and not to be disclosed to third parties. | No key in the repo or in fork builds by default. Each user brings their own. |
| 2.3 | Quotas are Overwolf's to set and change. | One batch request per update check, one per search page, a 15-minute in-memory cache. |
| 3.1 | No saving or caching of data obtained through the API. | Nothing from the API is written to disk except the project id and version name of an installed addon. The in-memory cache was upstream's choice; whether it is within 3.1 is unconfirmed. |
| 3.1 | No circumventing security mechanisms. | No scraping, no undocumented endpoints. Authors who turn off third-party distribution are never downloaded around: `allowModDistribution == false`, or a file with no `downloadUrl`, is link only. |
| 3.1 | The API may not be used to build a product that competes, directly or indirectly, with CurseForge. | **Open risk, not something code can settle.** See below. |

### Open questions and risks

1. **Will CurseForge issue a key for this use?** Keys come from the CurseForge
   for Studios console (<https://console.curseforge.com/>); the docs say people
   who are not game developers apply through a form and Overwolf reviews it.
   Clause 3.1 lets Overwolf refuse an addon manager as a competing product. It
   has refused one before: it declined WowUp's application, citing that clause
   and WowUp's partnership with Wago. Whether a personal key is granted,
   and whether it covers World of Warcraft, is decided by Overwolf per
   application. If the key is refused for WoW, `GET /v1/games/1` answers 403 and
   the app says the key was not accepted.
2. **Live use, verified 13 November 2026** with the owner's key, through the
   CLI against a throwaway game folder (`ADDONFORGE_STATE_DIR` plus `--cli use`):
   search (`search leatrix`: opted-out results come back `OptOut`, allowed ones
   resolve to an `edge.forgecdn.net` link); install of MixMap (project
   1753359), whose download carried the key and installed one folder; the
   update check (one batch request) reporting it up to date; after its
   recorded version was set back by hand, the check offering the update and
   `update` installing it; Leatrix Plus (94855, opted out) refused with the
   "only CurseForge's own app" message; a well-formed made-up key refused by
   CurseForge without touching the stored one. Not seen: a revoked key in use,
   and the status code the CDN returns for a bad key (the blog says 401; a
   request with no key got 403).
3. **Which hosts serve downloads.** Only `edge.forgecdn.net` then
   `mediafilez.forgecdn.net` was observed. If CurseForge ever redirects a
   download outside `forgecdn.net`, the install fails with "redirected away
   from CurseForge's own hosts" rather than sending the key along. That is
   deliberate; widen `key_zone` only for a host CurseForge documents.
4. **The built-in key path.** Kept, off by default. Do not use it for
   published builds without written permission from CurseForge.

### Error messages a user can meet

| Situation | Message |
|---|---|
| No key | "CurseForge needs an API key: add yours in Settings" |
| Key refused when saving | "CurseForge did not accept that key for World of Warcraft. …" (nothing is saved) |
| Key refused later (revoked, expired) | "CurseForge rejected the API key; check it in Settings" |
| Author opted out of third-party downloads | Row shows "CurseForge app only" (full reason on hover and in the details pane). Installing, or an installed row, says "Its author lets only CurseForge's own app download it, so AddonForge can't install (update) it", and points at the zip route: download the zip on its CurseForge page, then Install from zip (see [Local zip file](#local-zip-file)). CurseForge's `allowModDistribution` is `false` for these; checked on 12 November 2026 for NetherSuite and the rest of the Nether family, Details!, DBM, Bagnon, Leatrix Plus and Auctionator. |
| A newer release of an opted-out addon you have | "X is on CurseForge; you have Y. Open its page to download it, then use Install from zip." Read from the update check's batch; never a download link. |
| Search result with no Forever file | Row shows "no Forever file" |
| Nothing published for Forever | "CurseForge has no Forever release for this addon" |
| Download refused | "CurseForge refused the download: it did not accept the API key. Check the key in Settings" |
| Rate limit | "CurseForge rate limit hit; try again in a minute" |

### Sources

- [CurseForge 3rd Party API Terms and Conditions](https://support.curseforge.com/support/solutions/articles/9000207405)
- [CurseForge REST API documentation](https://docs.curseforge.com/rest-api)
- [Introducing API key authentication for CurseForge file downloads](https://blog.curseforge.com/introducing-api-key-authentication-for-curseforge-file-downloads/) (10 June 2026)
- [Update on CurseForge API](https://medium.com/overwolf/update-on-curseforge-api-180468476253) (Overwolf, on the WowUp application)

## Local zip file

### Why

Many CurseForge authors set `allowModDistribution: false` (the Nether family,
Details!, DBM, Bagnon, Leatrix Plus, Auctionator, and more). CurseForge then
gives no third-party app a download for them, whatever the key, and the fork
does not get around that: no scraping of CurseForge's website, no undocumented
endpoints, no download of any file whose author opted out (terms 3.1, above).
What remains is the user's own browser: they download the zip from the
addon's page themselves, and AddonForge installs that file safely and keeps
track of it.

### What the app does

- **Ways in:** Install from zip… in the My addons toolbar (the dialog plugin's
  `open`, filtered to `.zip`, several at once), dropping zips on the window
  (Tauri's `tauri://drag-drop` event; the webview gets no file path any other
  way), or `addonforge --cli install-zip <file.zip> [--force]`. Neither needed
  a capability beyond `core:default` and `dialog:default`, which the app
  already had.
- **Staging:** `install::stage_local` copies the zip into
  `%APPDATA%\AddonForge\downloads`, where downloads wait, under the same name
  rules and the same 512 MB cap. Only a regular file ending in `.zip` is taken.
  The copy is a new file (not `fs::copy`, which keeps the old modified time and
  would let the startup sweep take it for an abandoned download). The user's
  own file is never changed or removed.
- **Install:** the copy goes through `install::place_planned`, which is
  `place_locked` with one hook: under the addons lock, the extraction limits,
  name checks, safety scan and target-name checks all run first, exactly as
  for a download; then the hook reads the extracted TOCs; then folders move,
  with the same protection of other packages' folders, rollback and recovery.
  There is no second extraction path.
- **What it is:** the hook names the package the way the app already shows
  those folders: their managed owner, else a catalogue entry, else an id from
  the TOC (`curse:<id>` for `X-Curse-Project-ID`), else the folder name. So a
  pin carries over, a newer zip replaces the same package, and installing it
  later from a real source (GitHub, or CurseForge if the author allows it)
  takes over the same package. A zip with no Forever TOC answers `NOT_FOREVER`
  and the UI asks "install anyway?", as for a download.
- **Back to a real source:** the zip's key need not match the one a real
  source picks (a zip whose TOC names only a GitHub website is `folder:Foo`;
  a GitHub install of it is `github:owner/Foo`). So `install_from` lets a
  download take over a zip install (`takeover_plan`) when that zip install is
  the only package owning any of the archive's folders and the archive holds
  every one of its folders. The zip's record goes, its pin and ignore carry
  over, and every other package's folders stay protected.
- **Record:** `state.json` gets `source: "local"`, `source_id` the zip's file
  name, the TOC's version and the folders. That makes it managed: pinning,
  removal (deleted outright, as AddonForge's own) and recovery work as for any
  install.
- **Never updated:** a `local` record has no `Source`, whatever its TOC or the
  catalogue name: `pick_source` and `build_packages` give it none. Its row says
  "From a zip file" with the source "Zip file", and offers no Update or
  Reinstall; `update_package` and `--cli update` refuse with "installed from a
  zip file, so AddonForge can't update it". An exported list carries it with
  no source, so an import says it can't be installed automatically.
- **Newer on CurseForge:** if the TOC carries `X-Curse-Project-ID`, the
  package keeps that `curse_id`. With a CurseForge key, the update check's one
  batch request already includes it, and `curse_listing_into` reports "X is on
  CurseForge; you have Y. Open its page to download it, then use Install from
  zip." A listing that is not newer leaves the row's own note. Nothing is
  downloaded and nothing more is requested for it.

### Errors a user can meet

| Situation | Message |
|---|---|
| Not a `.zip` name | "choose a .zip file" (a drop of other files: "Only .zip files can be installed", or "N files were not a .zip and left alone") |
| Over 512 MB | "… is larger than 512 MB; refusing" |
| Not a zip inside | "not a valid zip: …" |
| Fails a check | The check's own reason, in full: "extract failed: archive has a file name Windows cannot store as written: …", "Refused for safety: …", "refusing suspicious folder name …", "the archive has two addon folders named …" |
| A folder belongs to another package | "The X folder is treated as a separate addon, so AddonForge left it in place." |
| No Forever TOC | "X.zip has no Forever TOC. … Install it anyway?" |
| Update asked for | "X was installed from a zip file, so AddonForge can't update it: download a newer zip and install it the same way" |

### Verified, and not

- Unit tests: staging (`install.rs`), the plan hook, and the whole flow in
  `lib.rs` `local_zip_tests`: managed record and row, catalogue identity,
  reinstall keeping the pin, a GitHub install taking over a `folder:` zip
  install (and refusing partial or mixed archives), another package's folder
  refused, eight malformed
  archives plus a non-zip refused with AddOns, the staging folder and the
  user's zip unchanged, the Forever question, and the CurseForge listing.
- `scripts/test-ui.js`: the dialog's filter, the install call, the row, the
  Forever question, a refusal, and drag-enter, leave and drop with Tauri's API
  stubbed.
- The CLI end to end (12 November 2026) against a throwaway `_forever_` folder
  and settings folder: install, scan, update refused, a newer zip over it with
  the pin kept, an executable refused, a classic-only zip asking first, remove.
  A screenshot of the window on the same throwaway profile showed the button
  and the row.
- Not driven by hand: the native file dialog and a real drop on the window.

### Known limits

- **Replacing a copy installed by hand deletes it.** An install that replaces
  a same-named folder AddonForge did not install deletes the old folder once
  it succeeds, rather than recycling it (AUDIT O3, an open owner decision).
  That is the usual case here: an opted-out addon was normally installed by
  hand before. Settings in `WTF` are not touched.
- A zip that holds only some of a package's folders takes over the package's
  record with just those folders; the others stay on disk and show under the
  same row, but Remove deletes only what the zip installed.
