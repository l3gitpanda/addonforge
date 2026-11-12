# Provider integrations

Where addons come from, what each source allows, and how the code treats it.
Status labels: Not started, In progress, Implemented, Tested, Blocked, Deferred.

| Provider | Code | Status | Access |
|---|---|---|---|
| GitHub releases | `src-tauri/src/sources/github.rs` | Implemented (upstream; not re-audited by the fork yet) | Public, no key. 60 API requests an hour without a token when pre-releases are on. |
| CurseForge | `src-tauri/src/sources/curseforge.rs` | Implemented; **live install not verified by the fork** (see below) | API key required. The user's own, or one built in. |
| WoWInterface | `src-tauri/src/sources/wowi.rs` | Implemented (upstream; not re-audited) | Public JSON API, no key. |
| TukUI | `src-tauri/src/sources/tukui.rs` | Implemented (upstream; not re-audited) | Public, no key. |
| Wago | `src-tauri/src/sources/wago.rs` | Link only | Upstream dropped it in build 25: Wago's updater keys are for its Patreon supporters. Not re-investigated by the fork yet. |
| GitLab, Codeberg, local zip, any HTTPS archive | none | Not started | |

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
| `api.curseforge.com` | Search, update check, resolving a file | The API. Header `x-api-key`. |
| `edge.forgecdn.net`, then the `forgecdn.net` host it redirects to (seen: `mediafilez.forgecdn.net`) | Downloading a file from a CurseForge project | CurseForge announced on 10 June 2026 that CDN downloads need the key from 16 July 2026, sent as `x-api-key`. |

Nowhere else. Two rules in code enforce it:

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
2. **Live install is unverified.** The fork has no key, so these were tested
   only as far as they go without one: a made-up key is refused by the real API
   and nothing is stored; a search with no key reports that a key is needed;
   the key round-trips through the real Windows Credential Manager. Since 12
   November 2026 the owner has a real key: it was accepted, and a CLI search
   with it works and reports each result's `allowModDistribution` correctly.
   Not yet seen by a session: a download that carries the key,
   the exact status code the CDN returns for a bad key (the blog says 401; a
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
| Author opted out of third-party downloads | Row shows "CurseForge app only" (full reason on hover and in the details pane). Installing, or an installed row, says "Its author lets only CurseForge's own app download it, so AddonForge can't install (update) it". CurseForge's `allowModDistribution` is `false` for these; checked on 12 November 2026 for NetherSuite and the rest of the Nether family, Details!, DBM, Bagnon, Leatrix Plus and Auctionator. |
| Search result with no Forever file | Row shows "no Forever file" |
| Nothing published for Forever | "CurseForge has no Forever release for this addon" |
| Download refused | "CurseForge refused the download: it did not accept the API key. Check the key in Settings" |
| Rate limit | "CurseForge rate limit hit; try again in a minute" |

### Sources

- [CurseForge 3rd Party API Terms and Conditions](https://support.curseforge.com/support/solutions/articles/9000207405)
- [CurseForge REST API documentation](https://docs.curseforge.com/rest-api)
- [Introducing API key authentication for CurseForge file downloads](https://blog.curseforge.com/introducing-api-key-authentication-for-curseforge-file-downloads/) (10 June 2026)
- [Update on CurseForge API](https://medium.com/overwolf/update-on-curseforge-api-180468476253) (Overwolf, on the WowUp application)
