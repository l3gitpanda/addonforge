# Fork roadmap

The working plan for turning AddonForge into a privacy-first fork. Kept current
so a new session can pick up where the last one stopped.

Status labels: **Not started**, **In progress**, **Implemented** (written, not
fully verified), **Tested** (verified, how is stated), **Blocked**, **Deferred**.
"Upstream" means the feature came with the original code. Where a row says
"not audited", nobody on the fork has read that code closely yet: do not treat
it as either sound or broken.

Last updated: 12 November 2026.

## Where things stand

- Forked from Topher121/AddonForge at v0.4.0 (build 31 source, commit `7bdd46b`).
  `origin` is the fork, `upstream` is the original.
- Stack: Rust + Tauri 2 (WebView2), plain HTML/JS/CSS in `ui/` with no
  framework and no bundler. About 8,000 lines of Rust and 1,400 of JS at the fork point.
- Baseline before any fork change: `cargo test` 97 passed, 1 ignored;
  `node scripts/security-check.mjs` OK; `node scripts/test-ui.js` OK.
- First fork change, on branch `curseforge-user-key`: a user-supplied
  CurseForge API key (see [CHANGELOG.md](../CHANGELOG.md) and
  [PROVIDER_INTEGRATIONS.md](PROVIDER_INTEGRATIONS.md)). After it:
  `cargo test` 106 passed, 2 ignored.

## Do first: the fork still behaves as the original

Found while testing the first change. None of these is fixed yet; each needs a
decision from the owner (the fork's name, repo address, and whether it signs
its own releases).

| # | Problem | Where | Effect |
|---|---|---|---|
| 1 | Self-update points at upstream | `selfupdate.rs` `LATEST_URL`, `signing.rs` `RELEASE_PUBLIC_KEY` | **A fork build offers "Update now" to upstream's build 31.** Accepting replaces the fork with the original app. Seen in a dev build on 12 Nov 2026. |
| 2 | Bug reports go to upstream | `lib.rs` `REPO_URL`, `SUPPORT_EMAIL`; Discord and Ko-fi links in `ui/index.html` | "Report a problem" opens an issue on the original repo or emails its maintainer about a build they did not make. |
| 3 | User-Agent names upstream | `lib.rs` `USER_AGENT` | Every request tells addon sites it is the original project. |
| 4 | Catalogue and stats come from upstream | `catalog.rs` `REMOTE_URL`, `STATS_URL` | Works, and is signature-checked with upstream's key. The fork cannot change the catalogue without its own key and URL. Also a launch-time request to a repo the fork does not control. |
| 5 | Same settings folder and app id | `state.rs` (`%APPDATA%\AddonForge`), `tauri.conf.json` `identifier` | The fork and the original share `state.json`. Either can rewrite the other's settings. |
| 6 | Daily stats workflow | `.github/workflows/stats.yml` | If Actions are enabled on the fork, it commits to `main` every day as "Pocket Forge Studios". |
| 7 | Version and build numbers | `Cargo.toml`, `tauri.conf.json`, `package.json`, `build.ps1` | Fork milestones (v0.1 to v1.0) need a scheme that cannot be confused with upstream's 0.4.0 build 31. |

Smallest safe step for #1 until the fork signs its own releases: set
`RELEASE_PUBLIC_KEY` handling so self-update is off, without breaking the
catalogue check that shares the key.

## Audit so far

Read closely by the fork:

| Area | Files | Finding |
|---|---|---|
| CurseForge | `sources/curseforge.rs`, parts of `lib.rs` | Search, install, update check and opt-out handling were complete but gated on a build-time key. Downloads did not send the key, which CurseForge's file servers now require. Both addressed. |
| Source model | `sources/mod.rs` | One `enum Source` with a `match` per operation; no provider trait. Fine for five sources, the thing to change for Phase Two. |
| Settings file | `state.rs` | Atomic write, `.bak` fallback, cross-process lock. Holds an unused `wago_key` field from before build 25. |
| Build-time key | `build.rs` | Plain constant in the binary. Off unless a key file or env var is supplied. |
| HTTP client | `lib.rs` `make_app` | One shared `reqwest` client: HTTPS only on every hop, 10 redirect limit, 120 s timeout. No per-provider switch, no offline mode, no request log. |
| Download | `install.rs` `download` | 512 MB cap, sanitised file name, staged outside the AddOns folder. |
| Extraction limits | `install.rs` `extract_limited` | 20,000 entries, 1 GiB expanded, `enclosed_name` for traversal. Rest of the install path not audited. |
| Webview | `tauri.conf.json`, `capabilities/default.json`, `scripts/security-check.mjs` | CSP `default-src 'self'`, no inline script, capabilities limited to core, opener, dialog. A CI gate fails the build if these weaken. |
| CI | `.github/workflows/build.yml` | Security check, `cargo test`, UI checks on Windows and macOS. No dependency audit, licence check, secret scan or clippy. |

Not audited yet (upstream; status unknown): `install.rs` beyond the two
functions above (safety scan, placement, rollback, junction handling),
`recycle.rs`, `backup.rs`, `selfupdate.rs`, `signing.rs`, `icons.rs` (remote
images), `wow.rs` and `wow/detection.rs`, `catalog.rs`, `log.rs`,
`sources/github.rs`, `wowi.rs`, `tukui.rs`, `wago.rs`, `build.ps1`, most of
`ui/app.js`.

### Outbound hosts (from the README and the code read so far)

`github.com`, `api.github.com`, `raw.githubusercontent.com` and GitHub's
download hosts; `api.curseforge.com`; `edge.forgecdn.net` and the
`forgecdn.net` host it redirects to; `api.mmoui.com` and the hosts it returns;
`api.tukui.org` and the hosts it returns; for preview images only:
`media.forgecdn.net`, wago.io hosts, `www.wowinterface.com`,
`cdn-wow.mmoui.com`. Needs confirming against the code in the Phase One audit.

At launch, by upstream's own description: catalogue, download totals, update
manifest, then an update check of every installed addon. No setting turns any
of it off.

## Phase One: privacy and security (P0)

| Item | Status | Notes |
|---|---|---|
| Fork identity (table above) | Not started | Needs the owner's decisions. Do before anything is released. |
| Offline mode, enforced in Rust | Not started | One gate in front of the shared client, not a UI switch. Scan, cached data, local management and backups must keep working. |
| Optional startup requests | Not started | Catalogue, stats, self-update check, addon update check: each switchable. |
| Per-provider network permissions | Not started | Same gate, keyed by provider or feature. |
| Network activity log and dashboard | Not started | Each request attributed to a provider or feature. |
| Update-check frequency | Not started | |
| Optional remote images | Not started | `icons.rs` fetches README images and avatars. |
| Credentials in the OS store | **Tested** for the CurseForge key | Unit test on an in-memory store; a round trip through the real Windows Credential Manager (`cargo test -- --ignored real_credential_store`). macOS Keychain path compiles on paper only: not built or run. |
| Credential migration from old settings | Not started | `wago_key` in `state.json` is unused: remove it, without breaking old files. |
| Diagnostics redaction | Implemented (upstream) + key never included | Home folder and usernames replaced. Not audited further. |
| Provider disclosures | In progress | CurseForge done in Settings and the README. |
| Zip traversal, bombs, limits, symlinks, junctions, Unicode and case collisions, temp files, races, rollback, deletion limits | Not audited | Upstream has tests for several. Audit, then add malformed-archive tests. |
| Update verification | Not audited | Ed25519 manifest signature + SHA-256 (upstream). |
| Redirect validation | **Tested** for CurseForge | Key-bearing requests cannot leave CurseForge's hosts. Other sources: HTTPS-only and a hop limit, nothing more. |
| External URL handling, CSP, webview boundary | Implemented (upstream), CI-gated | Not audited beyond the config. |
| Provider metadata validation | Not audited | |
| CI: `cargo audit`, licence policy, secret scan, clippy, malformed-archive tests | Not started | |
| SBOM, reproducible builds, provenance | Not started | Investigate. |

## Phase Two: unified sources (P1)

| Item | Status | Notes |
|---|---|---|
| Provider trait | Not started | Replace the `match` in `sources/mod.rs` with a trait: search, metadata, versions, resolve, download headers, limits. `curseforge::download_headers` is the first provider-specific download hook and shows the shape. |
| CurseForge | Implemented; live use **unverified** | Needs a real key. See PROVIDER_INTEGRATIONS.md for the open questions. |
| Wago | Not started | Re-check whether user-supplied keys are allowed. Link fallback exists. |
| GitLab, Codeberg, local zip, HTTPS archive | Not started | |
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
| `CHANGELOG.md` | Started |
| `docs/ARCHITECTURE.md`, `docs/SECURITY.md`, `docs/PRIVACY.md`, `docs/TESTING.md` | Not started |

## How to verify a change

```
node scripts/security-check.mjs          # webview and CSP gate
node scripts/test-ui.js                  # UI behaviour, no Tauri needed
cd src-tauri && cargo test               # Rust unit tests; nothing leaves this machine, no real WoW folder, no key
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

## Next, in order

1. Decide the fork's name, repo address and release signing; then fix the
   identity table above, self-update first.
2. Verify CurseForge end to end with a real key: search, install, update, an
   opted-out addon, a revoked key.
3. Phase One audit of `install.rs`, `recycle.rs`, `selfupdate.rs`, `icons.rs`,
   and the full list of outbound requests.
4. The network gate: offline mode, startup switches, per-provider permissions,
   activity log. One design, since they share an enforcement point.
5. CI: `cargo audit`, `cargo deny`, clippy, secret scanning.
