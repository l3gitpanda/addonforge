# Fork roadmap

The working plan for turning AddonForge into a privacy-first fork. Kept current
so a new session can pick up where the last one stopped.

Status labels: **Not started**, **In progress**, **Implemented** (written, not
fully verified), **Tested** (verified, how is stated), **Blocked**, **Deferred**.
"Upstream" means the feature came with the original code. Where a row says
"not audited", nobody on the fork has read that code closely yet: do not treat
it as either sound or broken.

Last updated: 13 November 2026.

## Where things stand

- Based on AddonForge v0.4.0 (build 31 source, commit `7bdd46b`).
  `origin` is the fork, `upstream` is the original.
- Stack: Rust + Tauri 2 (WebView2), plain HTML/JS/CSS in `ui/` with no
  framework and no bundler. About 8,000 lines of Rust and 1,400 of JS at the fork point.
- Baseline before any fork change: `cargo test` 97 passed, 1 ignored;
  `node scripts/security-check.mjs` OK; `node scripts/test-ui.js` OK.
- First fork change, on branch `curseforge-user-key`: a user-supplied
  CurseForge API key (see [CHANGELOG.md](../CHANGELOG.md) and
  [PROVIDER_INTEGRATIONS.md](PROVIDER_INTEGRATIONS.md)). After it:
  `cargo test` 106 passed, 2 ignored.
- Second change, same branch: self-update is off until the fork has its own
  update channel. After it: `cargo test` 110 passed, 2 ignored.
- Third change, branch `phase1-audit`: the Phase One audit of the install
  path, removal, self-update, images and every network read, with fixes and
  malformed-archive tests. Findings in [AUDIT.md](AUDIT.md). After it:
  `cargo test` 126 passed, 2 ignored.
- Fourth change, same branch: link-only rows name their reason ("CurseForge
  app only", "needs API key", "no Forever file", "Wago only") instead of a
  bare "link only", after an opted-out addon (NetherSuite) looked like a
  broken key. After it: `cargo test` 128 passed, 2 ignored; UI checks pass.
- Fifth change, branch `claude/festive-cori-e57496` (on top of
  `phase1-audit`): **install from a zip file**, for addons whose authors allow
  only CurseForge's own app. The user downloads the zip in a browser;
  AddonForge installs a copy through the same hardened path as a download and
  records it as managed with source `local`, never updated by the app. See
  [PROVIDER_INTEGRATIONS.md](PROVIDER_INTEGRATIONS.md#local-zip-file). After
  it: `cargo test` 140 passed, 2 ignored; UI checks pass; security check OK.
- CurseForge verified live with the owner's key (13 November 2026), through
  the CLI against a throwaway game folder: search, install (the download
  carries the key), update check, update, an opted-out addon refused, a
  made-up key refused without touching the stored one. Not seen: a revoked key
  in use. Details in [PROVIDER_INTEGRATIONS.md](PROVIDER_INTEGRATIONS.md).
- Sixth change, branch `network-gate`: **the network gate** (`net.rs`). Every
  request names its purpose and goes through one `Net`, which checks offline
  mode, the per-site and per-feature switches, and the purpose's hosts before
  the first request and on every redirect hop (closes AUDIT O1). Settings has a
  Network section with the switches and an in-memory activity list; there is
  `--cli network`. After it: `cargo test` 157 passed, 2 ignored; UI checks
  pass; security check OK. Merged as PR #7.
- Ninth change, branch `download-integrity`: **downloads are checked against
  the hash their source publishes** (AUDIT O2): CurseForge SHA-1, GitHub
  SHA-256, WoWInterface MD5; a mismatch is deleted and refused. And the
  unused `wago_key` is gone from `state.json`: old files still load, and the
  next save drops it from the file and its backup. After it: `cargo test` 171
  passed, 2 ignored.
- Eighth change, branch `upstream-sync`: **upstream merged through
  `73fdbd5`** (13 November 2026). Taken: CurseForge fingerprint matching
  (`fingerprint.rs`), QuestieDB as its own CurseForge project, 12 catalogue
  entries and download totals (catalogue byte-identical to upstream, so the
  signature verifies). Left out: upstream's own "install a downloaded ZIP"
  (`2f79b0a`), which duplicates the fork's zip install and clashed with it
  (same command name); its preview-before-install idea is noted below. The
  fingerprint request goes through the gate as CurseForge. After it:
  `cargo test` 164 passed, 2 ignored.
- Seventh change, branch `ci-hardening`: **CI hardening**. Clippy runs with
  warnings as errors on Windows and macOS; a new workflow (`checks.yml`) runs
  `cargo deny` (RustSec advisories, licences, bans, sources; policy in
  `src-tauri/deny.toml`) and a gitleaks scan of the whole history, on every PR,
  on main, and weekly. Its first run moved `zip` 2.4 to 9.5 (four advisories,
  none reachable from AddonForge's code) and `tokio` past an unsoundness fix.
  After it: `cargo test` 158 passed, 2 ignored; `cargo deny check` ok; clippy
  clean on Windows; gitleaks: no leaks in 149 commits.

## Do first: the fork still behaves as the original

Found while testing the first change. #1 is closed; the rest are open, and each
needs a decision from the owner (the fork's name, repo address, and whether it
signs its own releases).

| # | Problem | Where | Effect |
|---|---|---|---|
| 1 | ~~Self-update points at upstream~~ **Tested: off** | `selfupdate.rs` `UPDATE_CHANNEL` (`None`) | A fork build offered "Update now" to upstream's build 31 and would have replaced itself with the original app. Now no manifest is requested, nothing is offered and `apply` refuses. Verified by unit tests, by pointing the built CLI at upstream's real manifest with `--force` (refused, exe unchanged), and in the running app (no banner, no request in the log). **To turn self-update back on:** publish signed fork releases, put the fork's public key in `signing.rs`, and set `UPDATE_CHANNEL` to the fork's `latest.json`. The catalogue shares `RELEASE_PUBLIC_KEY` today, so changing the key needs #4 solved at the same time. `RELEASES_URL` (non-Windows) now names the fork's releases page and is unreachable while the channel is off. |
| 2 | ~~Bug reports go to upstream~~ **Implemented** | `lib.rs` `REPO_URL`; `ui/index.html` | `REPO_URL` is the fork. `SUPPORT_EMAIL`, the Report on GitHub and Email buttons, and the Discord and Ko-fi links are gone (the fork has issues off and is for private use). Settings keeps Copy diagnostics, which sends nothing. |
| 3 | ~~User-Agent names upstream~~ **Implemented** | `lib.rs` `USER_AGENT` | Now names `github.com/l3gitpanda/addonforge`. |
| 4 | Catalogue and stats come from upstream | `catalog.rs` `REMOTE_URL`, `STATS_URL` | **Partly done:** both URLs now point at the fork's `main`. The catalogue is still signature-checked with upstream's key, so it verifies only while `catalog/forever.json` and `.sig` are unchanged from upstream; editing the catalogue needs the fork's own key (the app falls back to the bundled copy when the signature fails). |
| 5 | Same settings folder and app id | `state.rs` (`%APPDATA%\AddonForge`), `tauri.conf.json` `identifier` | **Partly done:** `identifier` is now `com.l3gitpanda.addonforge`, so the installer and WebView data no longer collide with the original. `state.json` is still in `%APPDATA%\AddonForge`, shared with the original if both are installed. |
| 6 | Daily stats workflow | `.github/workflows/stats.yml` | **Implemented:** commits as `github-actions[bot]`. If Actions are enabled on the fork, it still commits to `main` every day. |
| 7 | Version and build numbers | `Cargo.toml`, `tauri.conf.json`, `package.json`, `build.ps1` | Fork milestones (v0.1 to v1.0) need a scheme that cannot be confused with upstream's 0.4.0 build 31. |

## Audit so far

Read closely by the fork:

| Area | Files | Finding |
|---|---|---|
| CurseForge | `sources/curseforge.rs`, parts of `lib.rs` | Search, install, update check and opt-out handling were complete but gated on a build-time key. Downloads did not send the key, which CurseForge's file servers now require. Both addressed. |
| Source model | `sources/mod.rs` | One `enum Source` with a `match` per operation; no provider trait. Fine for five sources, the thing to change for Phase Two. |
| Settings file | `state.rs` | Atomic write, `.bak` fallback, cross-process lock. Holds an unused `wago_key` field from before build 25. |
| Build-time key | `build.rs` | Plain constant in the binary. Off unless a key file or env var is supplied. |
| HTTP client | `net.rs` `Net` | One `reqwest` client per purpose, built on first use, behind the gate: HTTPS only on every hop, 10 redirect limit, 120 s timeout, the CurseForge key kept on CurseForge, each purpose held to its hosts on every hop, offline mode and switches, an in-memory activity list. Nothing else builds a client (tests aside). |
| Download | `install.rs` `download` | 512 MB cap, sanitised file name, staged outside the AddOns folder. Partial and abandoned zips are now removed (AUDIT A5). |
| Install path | `install.rs` | Extraction limits, safety scan, placement, rollback, recovery, junctions. Hardened: names Windows stores differently (alternate data streams, devices, trailing dots) and empty or duplicate target folders are refused. See [AUDIT.md](AUDIT.md). A zip from disk (`stage_local`, `place_planned`) goes through the same path; `place_planned` only adds a hook that runs after the checks and before anything moves. |
| Removal | `recycle.rs`, `install.rs` | Links unlinked before the shell call. Network-share paths fixed (A7). Open: replaced unmanaged folders are deleted, not recycled (O3); unverified behaviour on drives with no Recycle Bin (O6). |
| WTF backup | `backup.rs` | Atomic, skips links. Open: Documents may be synced to OneDrive (O5). |
| Self-update and signing | `selfupdate.rs`, `signing.rs` | Sound and dormant. Filename check tightened (U1). |
| Images and READMEs | `icons.rs` | Host list sound on the first URL; redirects not held to it (O1). Decoding now limited (N2). |
| Network reads | `net.rs` (new), every call site | Every reply body is now read with a size limit (N1). |
| Webview | `tauri.conf.json`, `capabilities/default.json`, `scripts/security-check.mjs` | CSP `default-src 'self'`, no inline script, capabilities limited to core, opener, dialog. A CI gate fails the build if these weaken. |
| CI | `.github/workflows/build.yml`, `checks.yml` | Security check, clippy (`-D warnings`), `cargo test --locked`, UI checks on Windows and macOS. `cargo deny` and a gitleaks history scan on Linux, on every PR and push to main and weekly. Third-party code in CI is pinned: actions by commit, cargo-deny and gitleaks by version and SHA-256. |

Not audited yet (upstream; status unknown): `wow.rs` and
`wow/detection.rs`, `log.rs`, `sources/github.rs` asset selection, `wowi.rs`,
`tukui.rs` and `wago.rs` beyond their HTTP calls, `build.ps1`, most of
`ui/app.js`.

### Outbound hosts (from the README and the code read so far)

Since the network gate, the list is in code: `Purpose::rules` in `net.rs`,
one list per purpose, applied to the first request and every redirect.

| Purpose | Hosts (a bare domain covers its subdomains) |
|---|---|
| Catalogue, download totals | `raw.githubusercontent.com` only |
| GitHub (and the dormant app update) | `github.com`, `githubusercontent.com` |
| Addon details (README, release notes) | `api.github.com`, `raw.githubusercontent.com` only |
| CurseForge | `api.curseforge.com` only, `forgecdn.net` |
| WoWInterface | `mmoui.com`, `wowinterface.com` |
| Tukui | `tukui.org` |
| Wago (dormant) | `wago.io` |
| Images | `github.com` only, `githubusercontent.com`, `media.forgecdn.net`, `wago.io`, `www.wowinterface.com`, `cdn-wow.mmoui.com` |

Checked on 13 November 2026 against what each site actually serves from:
GitHub release files go `github.com` to `release-assets.githubusercontent.com`,
tagged source zips to `codeload.github.com`, avatars to
`avatars.githubusercontent.com`; WoWInterface serves zips from
`cdn.wowinterface.com` and Tukui from `api.tukui.org`, neither redirecting. If a
site moves its downloads elsewhere, installs from it stop with "A ... request
tried to reach X, which is not one of the hosts it may use": widen the rule in
`Purpose::rules` only for a host the site really uses.

At launch: catalogue, download totals, then an update check of every installed
addon (the fork requests no update manifest). Each can be switched off in
Settings, Network, or all of it with offline mode.

## Phase One: privacy and security (P0)

| Item | Status | Notes |
|---|---|---|
| Fork identity (table above) | Not started | Needs the owner's decisions. Do before anything is released. |
| Offline mode, enforced in Rust | **Tested** | `net::Net` refuses every request before a connection is opened (`gate_tests::offline_mode_opens_no_connection`). The update check skips every row with a "Not checked" note and asks nothing (`network_gate_tests`); the catalogue is the built-in copy; zip installs, removal and backups need no network. CLI run: check, install and search all refused with one sentence. Window: screenshot only. |
| Optional startup requests | **Tested** | Catalogue refresh, download totals and the update check at launch each have a switch. The self-update check has none: it is dormant (no channel) and only offline mode governs it. |
| Per-provider network permissions | **Tested** | CurseForge, GitHub (including README and release notes), WoWInterface, Tukui. A site that is off keeps its images off too. A switched-off source is not swapped for another one: an addon whose source is off says so rather than silently updating from somewhere else. |
| Network activity log and dashboard | Implemented | In memory only: the last 300 requests, redirect hops and refusals with purpose, host, path (no query) and outcome, plus counts per purpose, in Settings, Network. Not a full dashboard: no byte counts, nothing kept across sessions. |
| Update-check frequency | Not started | Today: at launch (switchable) and by hand. |
| Optional remote images | **Tested** | One switch for avatars, catalogue icons and README pictures (AUDIT O8). Cached images still show; nothing is fetched. |
| Response size limits | **Tested** | `net.rs`; every reply body capped (AUDIT N1), remote image decoding limited (N2). |
| Credentials in the OS store | **Tested** for the CurseForge key | Unit test on an in-memory store; a round trip through the real Windows Credential Manager (`cargo test -- --ignored real_credential_store`). macOS Keychain path compiles on paper only: not built or run. |
| Credential migration from old settings | **Tested** | `wago_key` removed from `AppState`. An old file with one loads as before; the next save leaves it out of `state.json` and of `state.json.bak` (the backup is now written from the parsed settings, not copied). Test: `an_old_wago_key_loads_and_is_left_out_of_the_next_save`. |
| Diagnostics redaction | Implemented (upstream) + key never included | Home folder and usernames replaced. Not audited further. |
| Provider disclosures | In progress | CurseForge done in Settings and the README. |
| Zip traversal, bombs, limits, symlinks, junctions, Unicode and case collisions, temp files, races, rollback, deletion limits | **Tested** (audited, hardened) | Malformed-archive tests for alternate data streams, device names, trailing dots, bombs, entry counts, symlink entries, empty and duplicate target names, case collisions, partial downloads. Absolute, drive-letter, UNC and `..` entry names tested since the move to `zip` 9 (`absolute_drive_and_network_entry_names_never_leave_the_folder`). Open items O3, O4, O6 in AUDIT.md. |
| Update verification | Audited, dormant | Sound (AUDIT "Checked and sound"). Open: no freeze protection (O9). Decide with the fork's update channel. |
| Download integrity against provider hashes | **Tested** | `install::download` hashes as it streams; `Remote::hash` carries what the source published (`sources::FileHash`). Verified where published (see AUDIT O2); a missing hash is not an error, a wrong one is. |
| Redirect validation | **Tested** for every purpose | Each purpose's hosts on every hop (AUDIT O1, fixed), on top of HTTPS-only, a hop limit, and the CurseForge key kept on CurseForge's hosts. |
| External URL handling, CSP, webview boundary | Implemented (upstream), CI-gated | Not audited beyond the config. |
| Provider metadata validation | Not audited | |
| CI: `cargo audit`, licence policy, secret scan, clippy, malformed-archive tests | **Tested** | `cargo deny` covers the RustSec database (what `cargo audit` reads) as well as licences, bans and sources; six unmaintained-only crates under Tauri and `image-blp` are named exceptions in `deny.toml`. Malformed-archive tests run in `cargo test`. Verified locally (cargo-deny 0.20.2, gitleaks 8.30.1); the workflows' first run on GitHub is the PR's. Clippy on macOS is unverified until then. |
| SBOM, reproducible builds, provenance | Not started | Investigate. |

## Phase Two: unified sources (P1)

| Item | Status | Notes |
|---|---|---|
| Provider trait | Not started | Replace the `match` in `sources/mod.rs` with a trait: search, metadata, versions, resolve, download headers, limits. `curseforge::download_headers` is the first provider-specific download hook and shows the shape. |
| CurseForge | **Tested live** (13 November 2026) | With the owner's key, against a throwaway game folder. Not seen: a revoked key. See PROVIDER_INTEGRATIONS.md. |
| Wago | Not started | Re-check whether user-supplied keys are allowed. Link fallback exists. |
| Local zip | **Tested** | `--cli install-zip`, Install from zip… in My addons, or drop a zip on the window. Same install path as a download; recorded as managed, source `local`, never updated; a CurseForge id in its TOC keeps the "newer on CurseForge" notice. Verified by unit tests (`install.rs`, `lib.rs` `local_zip_tests`, malformed archives included), UI checks with Tauri stubbed, the CLI end to end against a throwaway game folder, and a screenshot of the window. Not driven by hand: the file dialog and a real drop. |
| GitLab, Codeberg, HTTPS archive | Not started | |
| Source choice per addon, preferences, dedupe, explicit source switching | Partly upstream | A source picker exists on catalogue rows. Not audited. |
| Release channels | Partly upstream | One global pre-release toggle. |

## Phases Three to Five

Not started, not audited. Upstream already has: pinning, a WTF backup,
missing-dependency detection with one-click install, list export/import,
starter packs, a CLI. Only WoW: Forever is supported; other clients are
detected and shown as unsupported.

## Documents

| Document | Status |
|---|---|
| `docs/FORK_ROADMAP.md` | This file |
| `docs/PROVIDER_INTEGRATIONS.md` | Started (CurseForge complete; others listed) |
| `docs/AUDIT.md` | Phase One audit: install path, removal, self-update, images, network reads |
| `CHANGELOG.md` | Started |
| `docs/ARCHITECTURE.md`, `docs/SECURITY.md`, `docs/PRIVACY.md`, `docs/TESTING.md` | Not started |

## How to verify a change

```
node scripts/security-check.mjs          # webview and CSP gate
node scripts/test-ui.js                  # UI behaviour, no Tauri needed
cd src-tauri && cargo test               # Rust unit tests; nothing leaves this machine, no real WoW folder, no key
cargo clippy --all-targets -- -D warnings # as CI runs it
cargo deny check                         # advisories, licences, bans, sources (needs cargo-deny: `cargo binstall cargo-deny`)
cargo test -- --ignored real_credential_store   # writes and removes one test entry in the OS credential store
```

To run the app without touching the real profile, point it at a throwaway
settings folder: set `ADDONFORGE_STATE_DIR`, then run
`src-tauri/target/debug/addonforge.exe`, or `--cli <command>` for headless use.
`scripts/dev-shot.ps1 -Exe <exe> -Out <png> -Tab settings` launches it,
screenshots the window and closes it. Two things to know: the credential store
is per Windows user, not per settings folder; and the app still finds and
selects the real WoW folder, which it reads but does not change until something
is installed.

Installs can be tried without the real game folder: make
`<dir>\_forever_\Interface\AddOns`, then, with `ADDONFORGE_STATE_DIR` set,
run `--cli use <dir>\_forever_` once. That throwaway profile then manages only
the throwaway folder, for the CLI and the window alike. The update check at
launch still uses the CurseForge key in the credential store, if there is one.

## Syncing with upstream

`git fetch upstream`, then `git log --oneline main..upstream/main` lists what
is new. Merge (not cherry-pick) on a branch, so git records what was taken
and the next sync starts after it. Keep the fork's side for its identity,
self-update, network gate and zip install; take `catalog/forever.json` and
its `.sig` byte for byte (the fork verifies them with upstream's key); route
any new request through `net::Net` with a `Purpose`, and run clippy, since
upstream's code is not linted with `-D warnings`.

Last synced: `73fdbd5`, 13 November 2026. Not taken: `2f79b0a` (upstream's
zip install). Worth borrowing from it later: it shows the folders a zip will
add or replace and asks before installing.

## Next, in order

1. Decide the fork's name, repo address and release signing; then fix the rest
   of the identity table above (bug-report address and User-Agent first: they
   send things to the original project on the fork's behalf).
2. ~~Verify CurseForge end to end with a real key.~~ Done 13 November 2026,
   except a revoked key (needs the owner to revoke one, or a second key).
3. ~~Phase One audit of `install.rs`, `recycle.rs`, `selfupdate.rs`,
   `icons.rs`, and the full list of outbound requests.~~ Done: see
   [AUDIT.md](AUDIT.md). Owner decisions it raised: O3 (recycle a replaced
   unmanaged folder, or ask first) and O5 (where WTF backups go by default).
4. ~~The network gate.~~ Done on branch `network-gate`: offline mode,
   startup switches, per-site switches, hosts on every redirect hop (O1),
   remote images (O8), activity list. Left from that item: update-check
   frequency, byte counts in the activity list.
5. ~~CI: `cargo audit`, `cargo deny`, clippy, secret scanning.~~ Done on
   branch `ci-hardening`. When a weekly run fails on a new advisory: update the
   crate if a fix exists (`cargo update -p <crate>`), otherwise add a named
   `ignore` entry with the reason to `src-tauri/deny.toml`. When the secret
   scan flags something that is not a secret, add its fingerprint (gitleaks
   prints it) with the reason to `.gitleaksignore`; never loosen the rules.
6. ~~Download integrity (AUDIT O2).~~ Done on branch `download-integrity`.
7. ~~Remove the unused `wago_key` from `state.json`.~~ Done, same branch.
8. Phase Two: the provider trait in `sources/` (search, metadata, resolve,
   download headers, published hash, limits), replacing the `match` per
   operation; then GitLab, Codeberg or HTTPS-archive sources on top of it.
9. Left over, small: update-check frequency; byte counts in the network
   activity list; a preview of the folders a zip will add or replace (from
   upstream's `2f79b0a`).
