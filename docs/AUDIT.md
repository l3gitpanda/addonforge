# Phase One audit: files, installs and the network

What the fork found reading the upstream code closely, what was changed, and
what is still open. Item 3 of "Next, in order" in
[FORK_ROADMAP.md](FORK_ROADMAP.md).

Audited 12 November 2026, against `main` at `6b9f1f1`. Files read in full:
`install.rs`, `recycle.rs`, `backup.rs`, `selfupdate.rs`, `signing.rs`,
`icons.rs`, `catalog.rs` (fetching), the install path in `lib.rs`
(`install_from`, `make_app`), and every HTTP call site in `sources/`.

Severity is for this app's situation: a desktop program run by one person,
installing archives from addon hosts. "High" means a hostile or broken archive
or server could change or destroy files the user cares about, or hide
something from the safety checks.

## Fixed in this change

| # | Finding | Severity | Fix | Test |
|---|---|---|---|---|
| A1 | A zip entry named `notes.lua:payload.exe` is written on NTFS as an alternate data stream of `notes.lua`. Directory listings, and so the safety scan, never see the stream. (Checked on this machine: the listing shows `notes.lua` only.) | High | Entry names are refused if any part contains `:` or other characters Windows does not allow. | `names_windows_would_store_differently_are_refused` |
| A2 | Device names (`CON`, `NUL`, `AUX`, `COM1`-`9`, `LPT1`-`9`, also with an extension) open the device on Windows 10 rather than create a file. Names ending in a dot or space are silently renamed by Windows, so two entries can land on one file. | Medium | Refused, on every platform, so one archive installs the same way everywhere. Ordinary names (`Console.lua`, `aux-addon`, `COM10.lua`) still pass. | `names_windows_...`, `ordinary_addon_names_are_not_mistaken_for_devices` |
| A3 | A folder whose only TOC is named `.toc` resolves to the empty folder name, which points the install at `AddOns` itself. Today the rename fails by luck (a folder cannot move into itself); nothing refused it. | High (latent) | Empty and unsafe target names are refused before anything moves. | `a_toc_with_no_name_cannot_point_the_install_at_addons_itself` |
| A4 | Two folders in one archive that resolve to the same name (`Shared-1.0/Shared.toc`, `Shared-2.0/Shared_Forever.toc`): the second replaced the first, and the install reported the name twice. With a folder already on disk, the outcome depended on the platform's rename rules. | Medium | Refused, case-insensitively, before anything moves. | `two_folders_with_one_target_name_are_refused`, `folders_that_differ_only_in_case_never_install_twice` |
| A5 | A download cut off or refused part-way (size cap, dropped connection) left its partial zip in `%APPDATA%\AddonForge\downloads` for good; so did a zip whose install never started (lock or task failure). Nothing ever cleaned that folder. | Low | The partial file is removed on failure; a declared size over 512 MB is refused before writing; at startup, zips in `downloads` older than 6 hours are removed (plain `.zip` files only). | `a_cut_off_download_leaves_no_file_behind`, `abandoned_downloads_are_cleared_and_nothing_else` |
| A6 | Recovery after a crash logged every successfully restored folder as "kept as a backup": the message was decided after the move, when the target always exists. | Low (wrong log) | Decided before the move; a failed move is now logged as such. | Not unit-tested (log text only). |
| A7 | Moving a folder to the Recycle Bin on a network share (`\\server\share\...`) stripped the `\\?\` prefix from `\\?\UNC\server\share\...`, leaving `UNC\server\share\...`: a path relative to the app's current folder. The shell call then failed (or, in theory, could hit a same-named relative path). | Medium | `\\?\UNC\` becomes `\\`; a path with no classic form (`\\?\Volume{...}`) is refused with a message. | `extended_paths_become_paths_the_shell_reads_the_same_way` |
| N1 | Seventeen reads of a reply had no size limit: CurseForge, GitHub, Wago, WoWInterface and Tukui API replies, `.pkgmeta`, README files and rendered README HTML, the catalogue and its signature, the download stats. A hostile or broken server could make the app hold any amount in memory. | Medium | One helper, `net.rs`: JSON 16 MB, text 2 MB, catalogue 4 MB; a declared length over the limit is refused before reading. Self-update and image reads, which already had caps, use the same helper. | `net::tests` (three, on a local socket) |
| N2 | Remote images (avatars, catalogue icons, README previews) were capped at 2 MB / 8 MB of bytes, but a small PNG can declare dimensions that need gigabytes to decode. | Medium | Decoding is limited to 8192 px a side and 256 MB of allocation. | `decode_tests` |
| U1 | Self-update (dormant): an update filename containing `:` (`C:x.exe`, drive-relative) or starting with `.` was accepted; joining it to the app folder could leave that folder. Only a correctly signed manifest could carry one. | Low | Refused. | Covered by the existing refusal path; no new test. |

## Checked and sound

- **Path traversal in archives.** The `zip` crate's `enclosed_name` refuses
  names that would leave the folder. Tested for `../`, and since the move to
  `zip` 9 (13 November 2026) for absolute, drive-letter, UNC and backslash
  `..` names too: each is refused, or (a leading `/`) read as relative and
  placed inside the folder. `zip` 9 also refuses a `:` in a name itself; the
  app checks the raw name first so the message stays specific.
- **Symlinks in archives.** A symlink entry is written as a plain file holding
  the link text; no link is ever created (new test
  `a_symlink_entry_is_written_as_a_plain_file`). The safety scan also refuses
  links, as a second check.
- **Zip bombs.** The 1 GiB expanded and 20,000-entry limits hold, and stop
  writing one byte past the limit rather than at the end of the entry (new
  tests with small limits).
- **Rollback.** Every replaced folder is moved aside first and put back on
  failure; a rollback that cannot complete keeps the backups for startup
  recovery. Recovery is skipped while another copy of the app holds the lock.
- **Junctions.** Rust's `remove_dir_all` removes a junction without following
  it (existing test). Before a folder goes to the Recycle Bin, every link
  inside it is unlinked so the shell never walks into SavedVariables.
- **Removal names.** `remove_folders` and `recycle_folders` refuse empty names,
  `..`, leading dots and separators.
- **Self-update** (dormant while the fork has no channel): Ed25519 signature
  over the exact manifest bytes, SHA-256 and size of the exe, a refusal when
  there is no checksum, a start marker before the old exe is given up, rollback
  on a failed start, and the old exe is deleted only if it is an AddonForge exe
  in the app's own folder.
- **Catalogue.** Used only with a valid signature; otherwise the bundled copy.
- **Remote image hosts.** The first URL is parsed properly (`user@host`,
  `?`/`#` tricks and look-alike suffixes are refused; existing test).
- **`IconTexture` paths.** Built one plain folder name at a time; drive
  letters, UNC paths and `..` are refused before the disk is touched.

## Open: needs a decision or belongs to a later item

| # | Finding | Severity | Why not fixed here |
|---|---|---|---|
| O1 | ~~**Redirects are not held to the allowed hosts.**~~ **Fixed 13 November 2026** by the network gate (`net.rs`): every request names its purpose and may only reach that purpose's hosts, checked before the first request and in the redirect callback on every hop. Tests: `net::policy_tests`, `net::gate_tests` (a redirect to another host is stopped before any connection to it). Was: the image host list was checked on the first URL only and the shared client followed up to 10 redirects to any `https` host, as did downloads from GitHub, Wago, WoWInterface and Tukui. | Medium | Fixed. |
| O2 | **Downloads are not checked against a provider hash.** CurseForge returns SHA-1/MD5 and a fingerprint per file, and GitHub release assets now carry a SHA-256 `digest`; neither is compared. TLS is the only integrity check. | Medium | Provider metadata validation, with the provider trait (Phase Two). |
| O3 | **Installing over an unmanaged folder deletes it permanently.** "Remove" sends folders AddonForge did not install to the Recycle Bin, but an install that replaces a same-named folder AddonForge did not install deletes the old one when the install succeeds. | Medium | Changes behaviour: options are to recycle the replaced folder, or to ask first. Owner's call. |
| O4 | **Links are unlinked before the Recycle Bin call, and stay unlinked if that call fails.** A `ForeverSVFixData` junction is removed from a folder that then stays where it was. | Low | The order is what keeps the shell out of SavedVariables. Recreating links on failure is possible but needs a design. |
| O5 | **WTF backups go to Documents**, which Windows often syncs to OneDrive. The archive holds account folder names and every addon's SavedVariables, so a backup can leave the machine without the user choosing that. | Medium (privacy) | Owner's call: a different default folder, a setting, or a note in the backup dialog. |
| O6 | **Recycle Bin on a drive without one.** The shell is asked to warn rather than delete outright (`FOF_WANTNUKEWARNING`), but with `FOF_NOERRORUI` and no window handle it is not verified that the warning appears. If it does not, a folder on such a drive (network share, some USB drives) is deleted permanently. | Medium | Needs a manual test on such a drive. |
| O7 | **Images from addon folders** (`IconTexture` TGA and BLP files) are decoded without the dimension limits remote images now have; `image-blp` 1 uses an older `image` crate. A hostile addon could make the icon scan use a lot of memory. | Low | The addon is already on disk and runs in the game; same fix as N2 when touching that code. |
| O8 | **GitHub avatars and README fetches reveal the installed addon list** to GitHub, at every launch, for every GitHub-sourced addon. | Privacy | **Mitigated 13 November 2026:** Settings, Network switches off remote images, or GitHub as a whole (which includes README and release-note fetches). On by default, so the default behaviour is unchanged. |
| O9 | **Self-update has no freeze protection.** A correctly signed but old `latest.json` can be replayed to keep a build from seeing newer releases. It cannot install an older build (`is_newer`). | Low | Dormant; decide when the fork's update channel is designed. |
| O10 | `--replaced <path>` on the command line makes the app delete that file, if it is an `AddonForge*.exe` in the app's own folder. Anyone who can run the app can already delete those files. | None | Recorded so it is not re-reported. |

## Not audited yet

`wow.rs`, `wow/detection.rs`, `log.rs`, `state.rs` beyond the earlier read,
`sources/github.rs` asset selection, `wowi.rs`/`tukui.rs`/`wago.rs` beyond
their HTTP calls, `build.ps1`, and most of `ui/app.js`.
