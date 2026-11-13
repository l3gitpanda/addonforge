//! Download a release zip and swap it into `Interface\AddOns` safely.
//!
//! Layout rules:
//!  - work happens inside `<AddOns>\.addonforge-tmp\` so the final step is a
//!    same-volume rename (atomic per folder), never a slow cross-drive copy;
//!  - existing folders are moved aside first and restored if anything fails;
//!  - the zip crate refuses paths that escape the extraction directory;
//!  - the extracted tree is scanned and refused if it contains anything
//!    executable (addons are Lua, XML, TOC and media, nothing else);
//!  - SavedVariables live in `WTF\`, which we never touch.

use anyhow::{anyhow, Context};
use futures_util::StreamExt;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub const TMP_DIR: &str = ".addonforge-tmp";

/// File types that have no business inside a WoW addon.
const FORBIDDEN_EXT: &[&str] = &[
    "exe", "dll", "sys", "scr", "com", "pif", "cpl", "msi", "msp", "bat", "cmd", "ps1", "psm1",
    "vbs", "vbe", "js", "jse", "wsf", "wsh", "hta", "jar", "py", "sh", "lnk", "reg", "inf",
    "ocx", "drv", "app", "dmg", "apk", "command", "dylib", "so", "pkg", "scpt", "tool", "terminal",
    "pyw", "mjs", "cjs", "url", "chm", "msc", "msix", "msixbundle", "appx", "appxbundle", "wsc", "ws",
    "psd1", "application", "gadget", "iso", "img", "vhd", "vhdx",
];

/// macOS bundles are directories; refuse folders with these extensions.
const FORBIDDEN_DIR_EXT: &[&str] = &[
    "app", "bundle", "framework", "kext", "plugin", "workflow", "appex", "prefpane", "saver",
];

/// Clock plus a counter: Update all downloads several addons at once, and two
/// can start in the same clock tick.
fn unique() -> String {
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    format!("{n:x}-{seq}")
}

/// Exclusive lock on the addons folder's temp area, shared by every copy of the
/// app. The OS drops it if the process dies, so it cannot go stale. `wait` false
/// returns None when another copy holds it.
fn lock_tmp(addons_dir: &Path, wait: bool) -> Option<fs::File> {
    let dir = addons_dir.join(TMP_DIR);
    fs::create_dir_all(&dir).ok()?;
    let file = fs::OpenOptions::new().create(true).write(true).truncate(false).open(dir.join("lock")).ok()?;
    if wait {
        file.lock().ok()?;
    } else if file.try_lock().is_err() {
        return None;
    }
    Some(file)
}

/// Run at startup. An install interrupted by a crash leaves the folders it
/// replaced in `<tmp>/<id>/old`. Put back each one whose place is now empty;
/// any other backup is moved to `<tmp>/recovered` so nothing is deleted. A work
/// folder is only removed once all its backups have been moved out.
pub fn clean_tmp(addons_dir: &Path) {
    // Another copy of the app may be mid-install; its folders are not abandoned.
    let Some(_guard) = lock_tmp(addons_dir, false) else {
        crate::logi!("another AddonForge is installing; skipping recovery");
        return;
    };
    let tmp = addons_dir.join(TMP_DIR);
    let Ok(entries) = fs::read_dir(&tmp) else { return };
    let recovered = tmp.join("recovered");
    for entry in entries.flatten() {
        let work = entry.path();
        if !work.is_dir() || work == recovered {
            continue;
        }
        let mut all_moved = true;
        if let Ok(backups) = fs::read_dir(work.join("old")) {
            for backup in backups.flatten() {
                let name = backup.file_name();
                let target = addons_dir.join(&name);
                // Decided before the move: afterwards the target exists either way.
                let occupied = target.exists();
                let moved = if occupied {
                    fs::create_dir_all(&recovered).is_ok()
                        && fs::rename(backup.path(), recovered.join(format!("{}-{}", entry.file_name().to_string_lossy(), name.to_string_lossy()))).is_ok()
                } else {
                    fs::rename(backup.path(), &target).is_ok()
                };
                crate::logi!(
                    "recovered interrupted install: {} ({})",
                    name.to_string_lossy(),
                    match (moved, occupied) {
                        (false, _) => "could not be moved; left in the temp folder",
                        (true, true) => "kept as a backup",
                        (true, false) => "restored",
                    }
                );
                all_moved &= moved;
            }
        }
        if all_moved {
            let _ = fs::remove_dir_all(&work);
        }
    }
}

/// How much an archive may hold. A small download must not be able to fill the
/// disk or the folder table.
struct ExtractLimits {
    entries: usize,
    bytes: u64,
}

const EXTRACT_LIMITS: ExtractLimits = ExtractLimits { entries: 20_000, bytes: 1 << 30 }; // 1 GiB expanded

/// A name Windows would not store as written: an alternate data stream
/// (`a.lua:x.exe` hides a second file that the safety scan never lists), a
/// device (`CON`, `NUL`, `COM1`, with or without an extension: Windows 10 opens
/// the device), a name it silently changes (a trailing dot or space), or
/// characters it does not allow. Refused on every platform, so one archive
/// installs the same way everywhere.
fn unsafe_name(name: &str) -> bool {
    if name.is_empty()
        || name.ends_with(['.', ' '])
        || name.chars().any(|c| c.is_control() || matches!(c, ':' | '<' | '>' | '"' | '|' | '?' | '*' | '\\' | '/'))
    {
        return true;
    }
    let stem = name.split('.').next().unwrap_or("").trim_end().to_ascii_uppercase();
    let numbered = |prefix: &str| stem.len() == 4 && stem.starts_with(prefix) && stem.as_bytes()[3].is_ascii_digit();
    matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$") || numbered("COM") || numbered("LPT")
}

/// Extract with limits, so a small archive cannot fill the disk. Entries that
/// would leave the destination, or that Windows would store under another name
/// or as something other than a plain file, are refused.
fn extract_limited(archive: &mut zip::ZipArchive<fs::File>, dest: &Path) -> anyhow::Result<()> {
    extract_with(archive, dest, &EXTRACT_LIMITS)
}

fn extract_with(archive: &mut zip::ZipArchive<fs::File>, dest: &Path, limits: &ExtractLimits) -> anyhow::Result<()> {
    use std::io::Read as _;
    let max_entries = limits.entries;
    if archive.len() > max_entries {
        anyhow::bail!("archive has {} files, more than the {max_entries} allowed", archive.len());
    }
    let mut total: u64 = 0;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let rel = entry.enclosed_name().ok_or_else(|| anyhow!("unsafe path in archive"))?;
        for part in rel.components() {
            if let std::path::Component::Normal(part) = part {
                if unsafe_name(&part.to_string_lossy()) {
                    anyhow::bail!("archive has a file name Windows cannot store as written: {}", entry.name());
                }
            }
        }
        let out = dest.join(rel);
        if entry.is_dir() {
            fs::create_dir_all(&out)?;
            continue;
        }
        if let Some(parent) = out.parent() {
            fs::create_dir_all(parent)?;
        }
        let remaining = limits.bytes - total;
        let mut file = fs::File::create(&out)?;
        let written = std::io::copy(&mut (&mut entry).take(remaining + 1), &mut file)?;
        if written > remaining {
            anyhow::bail!("archive expands past the {} MB limit", (limits.bytes >> 20).max(1));
        }
        total += written;
    }
    Ok(())
}

/// `headers` go with the request: a source that wants its API key on a
/// download says so here, and every other download sends none. `purpose` is
/// the source, so the network gate holds the download and every redirect to
/// that source's hosts.
pub async fn download(
    net: &crate::net::Net,
    purpose: crate::net::Purpose,
    url: &str,
    dest_dir: &Path,
    filename: &str,
    headers: &[(&'static str, String)],
) -> anyhow::Result<PathBuf> {
    fs::create_dir_all(dest_dir)?;
    let path = staged_path(dest_dir, filename);
    let mut request = net.get(purpose, url);
    for (name, value) in headers {
        request = request.header(name, value);
    }
    let resp = match request.send().await {
        Ok(resp) => resp,
        // The gate's own words say what was stopped and why; nothing was sent.
        Err(e) if e.is::<crate::net::Refused>() => return Err(e),
        Err(e) => return Err(e.context(format!("download failed: {url}"))),
    };
    let resp = resp
        .error_for_status()
        .with_context(|| format!("download refused: {url}"))?;
    if resp.content_length().is_some_and(|n| n > MAX_DOWNLOAD) {
        anyhow::bail!("download is larger than 512 MB; refusing");
    }
    let written = async {
        let mut file = fs::File::create(&path)?;
        let mut stream = resp.bytes_stream();
        let mut total: u64 = 0;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            total += chunk.len() as u64;
            if total > MAX_DOWNLOAD {
                anyhow::bail!("download exceeded 512 MB; refusing");
            }
            file.write_all(&chunk)?;
        }
        file.flush()?;
        Ok(())
    }
    .await;
    // A cut-off or refused download is never left behind half-written.
    if let Err(e) = written {
        let _ = fs::remove_file(&path);
        return Err(e);
    }
    Ok(path)
}

const MAX_DOWNLOAD: u64 = 512 * 1024 * 1024;

/// Where a zip waits for its install: `<dest_dir>/<unique>-<name>.zip`, with
/// the name reduced to plain ASCII. Used for downloads and for zips from disk.
fn staged_path(dest_dir: &Path, filename: &str) -> PathBuf {
    let safe: String = filename
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') { c } else { '_' })
        .collect();
    let safe = if safe.to_ascii_lowercase().ends_with(".zip") { safe } else { format!("{safe}.zip") };
    dest_dir.join(format!("{}-{}", unique(), safe))
}

/// Copy a zip the user chose into `dest_dir`, where downloads wait, and return
/// the copy. The install then reads, and afterwards deletes, its own copy:
/// never the user's file, which can change or vanish meanwhile. Same name
/// rules and the same 512 MB cap as a download.
pub fn stage_local(src: &Path, dest_dir: &Path) -> anyhow::Result<PathBuf> {
    stage_local_capped(src, dest_dir, MAX_DOWNLOAD)
}

fn stage_local_capped(src: &Path, dest_dir: &Path, max: u64) -> anyhow::Result<PathBuf> {
    use std::io::Read as _;
    let name = src.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    if !name.to_ascii_lowercase().ends_with(".zip") {
        anyhow::bail!("choose a .zip file ({name:?} is not one)");
    }
    let meta = fs::metadata(src).with_context(|| format!("cannot read {name}"))?;
    if !meta.is_file() {
        anyhow::bail!("{name} is not a file");
    }
    let too_big = || anyhow!("{name} is larger than {} MB; refusing", (max >> 20).max(1));
    if meta.len() > max {
        return Err(too_big());
    }
    fs::create_dir_all(dest_dir)?;
    let path = staged_path(dest_dir, &name);
    // A new file rather than fs::copy: Windows' copy keeps the original's
    // modified time, and the startup sweep would take an old zip for an
    // abandoned download while it is still being installed.
    let copied = (|| -> anyhow::Result<()> {
        let mut from = fs::File::open(src).with_context(|| format!("cannot read {name}"))?;
        let mut to = fs::File::create(&path)?;
        // The cap holds even if the file grows after it was measured.
        if std::io::copy(&mut (&mut from).take(max + 1), &mut to)? > max {
            return Err(too_big());
        }
        to.flush()?;
        Ok(())
    })();
    if let Err(e) = copied {
        let _ = fs::remove_file(&path);
        return Err(e);
    }
    Ok(path)
}

/// How long a staged download may sit before startup treats it as abandoned.
/// Long enough that another copy of the app still downloading is never disturbed.
const STALE_DOWNLOAD: std::time::Duration = std::time::Duration::from_secs(6 * 3600);

/// Run at startup. Remove staged zips that no install picked up (the app was
/// closed between download and install, or the install could not start).
/// Only plain `.zip` files are touched; folders and links are left alone.
pub fn clean_downloads(dir: &Path) -> usize {
    clean_downloads_older_than(dir, STALE_DOWNLOAD)
}

fn clean_downloads_older_than(dir: &Path, max_age: std::time::Duration) -> usize {
    let Ok(entries) = fs::read_dir(dir) else { return 0 };
    let mut removed = 0;
    for entry in entries.flatten() {
        let Ok(kind) = entry.file_type() else { continue };
        let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
        if !kind.is_file() || !name.ends_with(".zip") {
            continue;
        }
        let stale = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|m| m.elapsed().ok())
            .is_some_and(|age| age >= max_age);
        if stale && fs::remove_file(entry.path()).is_ok() {
            removed += 1;
        }
    }
    removed
}

/// The folder name WoW expects: the one its `.toc` is named after. A GitHub
/// source archive unpacks as `GatherLite-7.6.3/GatherLite.toc`, so the folder
/// has to become `GatherLite`. Multi-TOC addons (`X.toc`, `X_Forever.toc`)
/// resolve to the shortest stem.
fn canonical_name(dir: &Path) -> Option<String> {
    let current = dir.file_name()?.to_string_lossy().to_string();
    let mut stems: Vec<String> = fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter_map(|e| {
            let n = e.file_name().to_string_lossy().to_string();
            n.to_ascii_lowercase().ends_with(".toc").then(|| n[..n.len() - 4].to_string())
        })
        .collect();
    if stems.is_empty() || stems.iter().any(|s| s.eq_ignore_ascii_case(&current)) {
        return Some(current);
    }
    // Per-flavour TOCs share a base name: `GatherLite_Tbc`, `GatherLite_Vanilla`
    // -> `GatherLite`. Strip a known suffix, then take the most common base.
    const SUFFIXES: &[&str] = &[
        "forever", "camelot", "vanilla", "classic", "era", "anniversary", "tbc", "bcc", "wrath", "wotlk", "wotlkc",
        "cata", "mists", "mop", "standard", "mainline", "retail",
    ];
    let base = |stem: &str| -> String {
        let lower = stem.to_ascii_lowercase();
        for suf in SUFFIXES {
            for sep in ['_', '-'] {
                let tail = format!("{sep}{suf}");
                if lower.ends_with(&tail) && lower.len() > tail.len() {
                    return stem[..stem.len() - tail.len()].to_string();
                }
            }
        }
        stem.to_string()
    };
    let mut counts: Vec<(String, usize)> = Vec::new();
    for stem in &stems {
        let b = base(stem);
        match counts.iter_mut().find(|(name, _)| name.eq_ignore_ascii_case(&b)) {
            Some(entry) => entry.1 += 1,
            None => counts.push((b, 1)),
        }
    }
    counts.sort_by(|x, y| y.1.cmp(&x.1).then(x.0.len().cmp(&y.0.len())).then(x.0.cmp(&y.0)));
    stems.clear();
    Some(counts.remove(0).0)
}

fn has_toc(dir: &Path) -> bool {
    fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .any(|e| e.file_name().to_string_lossy().to_ascii_lowercase().ends_with(".toc"))
        })
        .unwrap_or(false)
}

/// Work out which extracted directories are addon folders.
fn addon_dirs(extracted: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let mut dirs: Vec<PathBuf> = fs::read_dir(extracted)?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    if dirs.is_empty() {
        anyhow::bail!("zip contains no addon folder");
    }
    // A wrapper directory (e.g. "MyAddon-1.2/") holding the real folders.
    if dirs.len() == 1 && !has_toc(&dirs[0]) {
        let inner: Vec<PathBuf> = fs::read_dir(&dirs[0])?
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_dir() && has_toc(p))
            .collect();
        if !inner.is_empty() {
            return Ok(inner);
        }
    }
    let with_toc: Vec<PathBuf> = dirs.iter().filter(|d| has_toc(d)).cloned().collect();
    if with_toc.is_empty() {
        anyhow::bail!("zip has folders but none contains a .toc file");
    }
    Ok(with_toc)
}

/// Walk the extracted tree; refuse anything executable. Returns file count.
pub fn safety_scan(root: &Path) -> anyhow::Result<usize> {
    fn walk(root: &Path, dir: &Path, depth: usize, count: &mut usize) -> anyhow::Result<()> {
        if depth > 32 {
            anyhow::bail!("zip nests folders absurdly deep");
        }
        for e in fs::read_dir(dir)?.flatten() {
            let p = e.path();
            let ft = e.file_type()?;
            if ft.is_symlink() {
                anyhow::bail!("zip contains a symbolic link: {}", p.display());
            }
            if ft.is_dir() {
                let lower = e.file_name().to_string_lossy().to_ascii_lowercase();
                if lower.contains('.') && FORBIDDEN_DIR_EXT.contains(&lower.rsplit('.').next().unwrap_or("")) {
                    anyhow::bail!(
                        "SAFETY: refused, the zip contains an application bundle ({}) which no WoW addon needs",
                        p.strip_prefix(root).unwrap_or(&p).display()
                    );
                }
                walk(root, &p, depth + 1, count)?;
                continue;
            }
            *count += 1;
            let name = e.file_name().to_string_lossy().to_string();
            let lower = name.to_ascii_lowercase();
            // "foo.lua.exe" style double extensions are caught by taking the last one.
            let ext = lower.rsplit('.').next().unwrap_or("");
            if lower.contains('.') && FORBIDDEN_EXT.contains(&ext) {
                anyhow::bail!(
                    "SAFETY: refused, the zip contains an executable file ({}) which no WoW addon needs",
                    p.strip_prefix(root).unwrap_or(&p).display()
                );
            }
            // Windows and macOS also run by "magic": check the first bytes of anything unusual.
            if !matches!(ext, "lua" | "xml" | "toc" | "tga" | "blp" | "png" | "jpg" | "jpeg" | "gif" | "ttf" | "otf" | "mp3" | "ogg" | "wav" | "txt" | "md" | "json" | "html" | "css" | "csv") {
                // Fail closed: a file we can't read (e.g. mode 0o111) can't be vetted.
                let unreadable = || anyhow!("SAFETY: refused, cannot inspect {}", p.strip_prefix(root).unwrap_or(&p).display());
                let mut f = fs::File::open(&p).map_err(|_| unreadable())?;
                let mut head = [0u8; 4];
                use std::io::Read;
                let n = f.read(&mut head).map_err(|_| unreadable())?;
                if n >= 2 && &head[..2] == b"MZ" {
                    anyhow::bail!(
                        "SAFETY: refused, {} is a Windows executable in disguise",
                        p.strip_prefix(root).unwrap_or(&p).display()
                    );
                }
                if n >= 2 && &head[..2] == b"#!" {
                    anyhow::bail!(
                        "SAFETY: refused, {} is a script in disguise",
                        p.strip_prefix(root).unwrap_or(&p).display()
                    );
                }
                // Mach-O 32/64-bit (both byte orders) and universal (32/64-bit) binaries.
                if n == 4
                    && matches!(
                        head,
                        [0xFE, 0xED, 0xFA, 0xCE | 0xCF] | [0xCE | 0xCF, 0xFA, 0xED, 0xFE] | [0xCA, 0xFE, 0xBA, 0xBE | 0xBF]
                    )
                {
                    anyhow::bail!(
                        "SAFETY: refused, {} is a macOS executable in disguise",
                        p.strip_prefix(root).unwrap_or(&p).display()
                    );
                }
            }
        }
        Ok(())
    }
    let mut count = 0;
    walk(root, root, 0, &mut count)?;
    Ok(count)
}

/// Extract `zip_path` and move its addon folders into `addons_dir`,
/// replacing same-named folders. Returns the folder names installed.
/// `protected_folders` maps lowercase folder names to their owning package;
/// ownership only prevents replacing folders that are still present on disk.
/// Take the addons lock and hold it until the returned file is dropped. Blocks
/// while another copy of the app holds it.
pub fn lock_addons(addons_dir: &Path) -> anyhow::Result<fs::File> {
    lock_tmp(addons_dir, true).ok_or_else(|| anyhow!("could not lock the addons folder"))
}

#[cfg_attr(not(test), allow(dead_code))]
pub fn place(
    zip_path: &Path,
    addons_dir: &Path,
    protected_folders: &HashMap<String, String>,
) -> anyhow::Result<Vec<String>> {
    // Held until this install has finished or rolled back, so no other copy of
    // the app can recover or remove its folders in the meantime.
    let _guard = lock_addons(addons_dir)?;
    place_locked(zip_path, addons_dir, protected_folders)
}

/// Like `place`, for callers that already hold the addons lock.
pub fn place_locked(
    zip_path: &Path,
    addons_dir: &Path,
    protected_folders: &HashMap<String, String>,
) -> anyhow::Result<Vec<String>> {
    place_planned(zip_path, addons_dir, |_| Ok((protected_folders.clone(), ()))).map(|(installed, ())| installed)
}

/// One addon folder found in an archive that passed extraction and the
/// safety scan: where it was extracted, and the name it will be installed as.
pub type Found = (PathBuf, String);

/// Like `place_locked`, for an install that can only decide what it is once it
/// has seen the archive (a zip from disk: which package it is, whether it is
/// for Forever). `plan` runs after extraction, the safety scan and the folder
/// name checks, and before anything in AddOns moves. It returns the folder
/// owners to protect, plus whatever the caller wants back; an error from it
/// stops the install with nothing changed. The caller holds the addons lock.
pub fn place_planned<T>(
    zip_path: &Path,
    addons_dir: &Path,
    plan: impl FnOnce(&[Found]) -> anyhow::Result<(HashMap<String, String>, T)>,
) -> anyhow::Result<(Vec<String>, T)> {
    fs::create_dir_all(addons_dir)?;
    let work = addons_dir.join(TMP_DIR).join(unique());
    let extract_dir = work.join("x");
    let backup_dir = work.join("old");
    fs::create_dir_all(&extract_dir)?;
    fs::create_dir_all(&backup_dir)?;

    // Set when a rollback could not put every folder back. The backups are then
    // kept in the work folder for clean_tmp to recover on the next start.
    let keep_backups = std::cell::Cell::new(false);

    let result = (|| -> anyhow::Result<(Vec<String>, T)> {
        let file = fs::File::open(zip_path)?;
        let mut archive = zip::ZipArchive::new(file).context("not a valid zip")?;
        extract_limited(&mut archive, &extract_dir).context("extract failed")?;
        let files = safety_scan(&extract_dir)?;
        crate::logi!("zip ok: {} files, no executables", files);

        // Compare actual names case-insensitively, including on case-sensitive volumes.
        let on_disk: HashSet<String> = fs::read_dir(addons_dir)?
            .map(|entry| entry.map(|e| e.file_name().to_string_lossy().to_ascii_lowercase()))
            .collect::<std::io::Result<_>>()?;
        // Resolve every final target before moving any installed folder aside.
        let mut targets: HashSet<String> = HashSet::new();
        let sources: Vec<Found> = addon_dirs(&extract_dir)?
            .into_iter()
            .map(|src| {
                let original = src.file_name().ok_or_else(|| anyhow!("bad folder name"))?
                    .to_string_lossy().to_string();
                let name = canonical_name(&src).unwrap_or(original);
                // A file called just `.toc` gives an empty name, which would point at AddOns itself.
                if name.is_empty() || name.starts_with('.') || name.contains("..") || unsafe_name(&name) {
                    anyhow::bail!("refusing suspicious folder name {name:?}");
                }
                // Two folders landing on one name: the second would replace the first.
                if !targets.insert(name.to_ascii_lowercase()) {
                    anyhow::bail!("the archive has two addon folders named {name}; refusing to guess which one is meant");
                }
                Ok((src, name))
            })
            .collect::<anyhow::Result<_>>()?;
        let (protected_folders, planned) = plan(&sources)?;
        for (_, name) in &sources {
            let lower = name.to_ascii_lowercase();
            if on_disk.contains(&lower) {
                if let Some(owner) = protected_folders.get(&lower) {
                    anyhow::bail!("cannot replace addon folder {name}: it belongs to {owner}");
                }
            }
        }
        let mut moved: Vec<(PathBuf, PathBuf)> = Vec::new(); // (target, backup)
        let mut installed = Vec::new();

        let restore = |moved: &Vec<(PathBuf, PathBuf)>| {
            for (target, backup) in moved.iter().rev() {
                let _ = fs::remove_dir_all(target);
                if fs::rename(backup, target).is_err() && backup.exists() {
                    keep_backups.set(true);
                }
            }
        };

        for (src, name) in sources {
            let target = addons_dir.join(&name);
            let backup = backup_dir.join(&name);
            if target.exists() {
                if let Err(e) = fs::rename(&target, &backup) {
                    restore(&moved);
                    return Err(anyhow!("could not move aside {name}: {e} (is WoW running?)"));
                }
                moved.push((target.clone(), backup));
            } else {
                moved.push((target.clone(), backup));
            }
            if let Err(e) = fs::rename(&src, &target) {
                restore(&moved);
                return Err(anyhow!("could not install {name}: {e}"));
            }
            installed.push(name);
        }
        Ok((installed, planned))
    })();

    if !keep_backups.get() {
        let _ = fs::remove_dir_all(&work);
    } else {
        crate::loge!("install rollback incomplete; kept backups in {} for recovery", work.display());
    }
    let _ = fs::remove_file(zip_path);
    // Leave the tmp root tidy if empty.
    let _ = fs::remove_dir(addons_dir.join(TMP_DIR));
    result
}

/// Delete the given addon folders (used by "Remove").
#[cfg_attr(not(test), allow(dead_code))]
pub fn remove_folders(addons_dir: &Path, folders: &[String]) -> anyhow::Result<Vec<String>> {
    // Same lock as installs, so a removal cannot race an install in another copy.
    let _guard = lock_addons(addons_dir)?;
    remove_folders_locked(addons_dir, folders)
}

/// Like `remove_folders`, for callers that already hold the addons lock.
pub fn remove_folders_locked(addons_dir: &Path, folders: &[String]) -> anyhow::Result<Vec<String>> {
    let mut gone = Vec::new();
    for f in folders {
        if f.is_empty() || f.starts_with('.') || f.contains("..") || f.contains(['/', '\\']) {
            anyhow::bail!("refusing to remove suspicious folder name {f}");
        }
        let p = addons_dir.join(f);
        if p.is_dir() {
            fs::remove_dir_all(&p).with_context(|| format!("remove {f}"))?;
            gone.push(f.clone());
        }
    }
    Ok(gone)
}

/// Move the given addon folders to the Recycle Bin / Trash instead of deleting
/// them. Used for addons AddonForge did not install: it never knew what was in
/// them, so the removal has to be reversible. Caller holds the addons lock.
pub fn recycle_folders_locked(addons_dir: &Path, folders: &[String]) -> anyhow::Result<Vec<String>> {
    let mut paths = Vec::new();
    let mut gone = Vec::new();
    for f in folders {
        if f.is_empty() || f.starts_with('.') || f.contains("..") || f.contains(['/', '\\']) {
            anyhow::bail!("refusing to remove suspicious folder name {f}");
        }
        let p = addons_dir.join(f);
        let Ok(meta) = fs::symlink_metadata(&p) else { continue };
        if meta.file_type().is_symlink() {
            anyhow::bail!("{f} is a link, not an addon folder, so it was left alone");
        }
        if !meta.is_dir() {
            continue;
        }
        // The shell's delete is not our code: never hand it a tree that can
        // lead out of AddOns (ForeverSVFix links SavedVariables into folders).
        let links = crate::recycle::unlink_reparse_points(&p).with_context(|| format!("check {f} for links"))?;
        if links > 0 {
            crate::logi!("unlinked {links} link(s) inside {f} before moving it to the {}", crate::recycle::label());
        }
        paths.push(p);
        gone.push(f.clone());
    }
    if paths.is_empty() {
        return Ok(gone);
    }
    crate::recycle::to_trash(&paths).map_err(|e| anyhow::anyhow!("{e}"))?;
    Ok(gone)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addon_zip(root: &Path, files: &[(&str, &str)]) -> PathBuf {
        fs::create_dir_all(root).unwrap();
        let path = root.join("release.zip");
        let mut archive = zip::ZipWriter::new(fs::File::create(&path).unwrap());
        for (name, contents) in files {
            archive.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
            archive.write_all(contents.as_bytes()).unwrap();
        }
        archive.finish().unwrap();
        path
    }

    #[test]
    fn protected_folder_prevents_changes_to_every_installed_folder() {
        let root = std::env::temp_dir().join(format!("af-owned-{}", unique()));
        let addons = root.join("AddOns");
        for name in ["AAddon", "ZDatabase"] {
            fs::create_dir_all(addons.join(name)).unwrap();
            fs::write(addons.join(name).join("keep.lua"), format!("original {name}")).unwrap();
        }
        let zip = addon_zip(&root, &[
            ("AAddon/AAddon.toc", "## Version: 2\n"),
            ("AAddon/new.lua", "new primary"),
            ("ZDATABASE/ZDatabase.toc", "## Version: 2\n"),
            ("ZDATABASE/new.lua", "new database"),
        ]);
        let protected = HashMap::from([("zdatabase".into(), "github:author/Database".into())]);
        let error = place(&zip, &addons, &protected).unwrap_err().to_string();
        assert!(error.contains("ZDATABASE"), "{error}");
        assert!(error.contains("github:author/Database"), "{error}");
        for name in ["AAddon", "ZDatabase"] {
            assert_eq!(fs::read_to_string(addons.join(name).join("keep.lua")).unwrap(), format!("original {name}"));
            assert!(!addons.join(name).join("new.lua").exists());
            assert!(!addons.join(name).join(format!("{name}.toc")).exists());
        }
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn ownership_guard_checks_the_canonical_source_archive_name() {
        let root = std::env::temp_dir().join(format!("af-owned-canon-{}", unique()));
        let addons = root.join("AddOns");
        fs::create_dir_all(addons.join("SharedData")).unwrap();
        fs::write(addons.join("SharedData").join("keep.lua"), "keep database").unwrap();
        let zip = addon_zip(&root, &[
            ("Project-1.2/SharedData_Forever.toc", "## Version: 2\n"),
        ]);
        let protected = HashMap::from([("shareddata".into(), "folder:SharedData".into())]);
        let error = place(&zip, &addons, &protected).unwrap_err().to_string();
        assert!(error.contains("SharedData"), "{error}");
        assert!(error.contains("folder:SharedData"), "{error}");
        assert_eq!(fs::read_to_string(addons.join("SharedData").join("keep.lua")).unwrap(), "keep database");
        assert!(!addons.join("SharedData").join("SharedData_Forever.toc").exists());
        assert!(!addons.join("Project-1.2").exists());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn missing_protected_folder_can_be_installed_but_is_protected_once_present() {
        let root = std::env::temp_dir().join(format!("af-owned-missing-{}", unique()));
        let addons = root.join("AddOns");
        let protected = HashMap::from([("shareddata".into(), "github:author/Database".into())]);
        let zip = addon_zip(&root, &[
            ("Project-1.2/SharedData_Forever.toc", "## Version: 1\n"),
            ("Project-1.2/data.lua", "original database"),
        ]);
        let installed = place(&zip, &addons, &protected).unwrap();
        assert_eq!(installed, vec!["SharedData"]);
        assert_eq!(fs::read_to_string(addons.join("SharedData").join("SharedData_Forever.toc")).unwrap(), "## Version: 1\n");
        assert_eq!(fs::read_to_string(addons.join("SharedData").join("data.lua")).unwrap(), "original database");
        assert!(!addons.join("Project-1.2").exists());

        fs::create_dir_all(addons.join("AAddon")).unwrap();
        fs::write(addons.join("AAddon").join("keep.lua"), "original primary").unwrap();
        let zip = addon_zip(&root, &[
            ("AAddon/AAddon.toc", "## Version: 2\n"),
            ("AAddon/new.lua", "new primary"),
            ("Project-2.0/SharedData_Forever.toc", "## Version: 2\n"),
            ("Project-2.0/data.lua", "new database"),
        ]);
        let error = place(&zip, &addons, &protected).unwrap_err().to_string();
        assert!(error.contains("SharedData"), "{error}");
        assert!(error.contains("github:author/Database"), "{error}");
        assert_eq!(fs::read_to_string(addons.join("AAddon").join("keep.lua")).unwrap(), "original primary");
        assert!(!addons.join("AAddon").join("new.lua").exists());
        assert!(!addons.join("AAddon").join("AAddon.toc").exists());
        assert_eq!(fs::read_to_string(addons.join("SharedData").join("SharedData_Forever.toc")).unwrap(), "## Version: 1\n");
        assert_eq!(fs::read_to_string(addons.join("SharedData").join("data.lua")).unwrap(), "original database");
        assert!(!addons.join("Project-2.0").exists());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn addon_folders_are_installed_under_canonical_names() {
        let root = std::env::temp_dir().join(format!("af-folders-{}", unique()));
        let addons = root.join("AddOns");
        fs::create_dir_all(addons.join("Core")).unwrap();
        fs::write(addons.join("Core").join("old.lua"), "old primary").unwrap();
        let zip = addon_zip(&root, &[
            ("Bundle-1.2/Core-2.0/Core_Forever.toc", "## Version: 2\n"),
            ("Bundle-1.2/Core-2.0/new.lua", "new primary"),
            ("Bundle-1.2/Data/Data.toc", "## Version: 3\n"),
            ("Bundle-1.2/Data/new.lua", "new database"),
        ]);
        let mut installed = place(&zip, &addons, &HashMap::new()).unwrap();
        installed.sort();
        assert_eq!(installed, vec!["Core", "Data"]);
        assert_eq!(fs::read_to_string(addons.join("Core").join("Core_Forever.toc")).unwrap(), "## Version: 2\n");
        assert_eq!(fs::read_to_string(addons.join("Data").join("Data.toc")).unwrap(), "## Version: 3\n");
        assert_eq!(fs::read_to_string(addons.join("Core").join("new.lua")).unwrap(), "new primary");
        assert_eq!(fs::read_to_string(addons.join("Data").join("new.lua")).unwrap(), "new database");
        assert!(!addons.join("Core").join("old.lua").exists());
        assert!(!addons.join("Core-2.0").exists());
        assert!(!addons.join("Bundle-1.2").exists());
        fs::remove_dir_all(&root).unwrap();
    }

    /// ForeverSVFix puts a directory link inside addon folders that points at
    /// the user's SavedVariables. Replacing or cleaning up an addon folder must
    /// remove the link, never what it points to.
    #[test]
    #[cfg(windows)]
    fn removing_a_folder_never_follows_a_junction() {
        let root = std::env::temp_dir().join(format!("af-junction-{}", unique()));
        let saved = root.join("WTF").join("SavedVariables");
        fs::create_dir_all(&saved).unwrap();
        fs::write(saved.join("Addon.lua"), "keep me").unwrap();
        let addon = root.join("AddOns").join("Addon");
        fs::create_dir_all(&addon).unwrap();
        let link = addon.join("ForeverSVFixData");
        let made = std::process::Command::new("cmd")
            .args(["/c", "mklink", "/J", &link.to_string_lossy(), &saved.to_string_lossy()])
            .output()
            .unwrap();
        assert!(made.status.success(), "mklink failed: {}", String::from_utf8_lossy(&made.stderr));
        assert!(link.join("Addon.lua").exists(), "junction should expose the file");
        fs::remove_dir_all(&addon).unwrap();
        assert!(!addon.exists());
        assert!(saved.join("Addon.lua").exists(), "the linked SavedVariables file must survive");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn safety_scan_refuses_executables() {
        let root = std::env::temp_dir().join(format!("af-safety-{}", unique()));
        let addon = root.join("Evil");
        fs::create_dir_all(&addon).unwrap();
        fs::write(addon.join("Evil.toc"), "## Interface: 16001\n").unwrap();
        fs::write(addon.join("Evil.lua"), "print(1)\n").unwrap();
        assert!(safety_scan(&root).is_ok());
        fs::write(addon.join("helper.exe"), b"MZ\0\0").unwrap();
        let err = safety_scan(&root).unwrap_err().to_string();
        assert!(err.contains("SAFETY"), "{err}");
        fs::remove_file(addon.join("helper.exe")).unwrap();
        // Disguised: no extension, MZ header.
        fs::write(addon.join("README"), b"MZ\x90\x00").unwrap();
        assert!(safety_scan(&root).is_err());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn source_archive_folder_takes_its_toc_name() {
        let root = std::env::temp_dir().join(format!("af-canon-{}", unique()));
        let wrapped = root.join("GatherLite-7.6.3");
        fs::create_dir_all(&wrapped).unwrap();
        fs::write(wrapped.join("GatherLite.toc"), "## Interface: 16001\n").unwrap();
        fs::write(wrapped.join("GatherLite_Forever.toc"), "## Interface: 16001\n").unwrap();
        assert_eq!(canonical_name(&wrapped).as_deref(), Some("GatherLite"));
        // Only per-flavour TOCs, no plain one (GatherLite 7.6.3 on GitHub).
        let flavours = root.join("GatherLite-7.6.4");
        fs::create_dir_all(&flavours).unwrap();
        for f in ["GatherLite_Cata.toc", "GatherLite_Tbc.toc", "GatherLite_Vanilla.toc", "GatherLite_Wrath.toc"] {
            fs::write(flavours.join(f), "").unwrap();
        }
        assert_eq!(canonical_name(&flavours).as_deref(), Some("GatherLite"));
        let proper = root.join("BigWigs");
        fs::create_dir_all(&proper).unwrap();
        fs::write(proper.join("BigWigs.toc"), "").unwrap();
        fs::write(proper.join("BigWigs_Forever.toc"), "").unwrap();
        assert_eq!(canonical_name(&proper).as_deref(), Some("BigWigs"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn safety_scan_refuses_mach_o() {
        let root = std::env::temp_dir().join(format!("af-macho-{}", unique()));
        let addon = root.join("Evil");
        fs::create_dir_all(&addon).unwrap();
        fs::write(addon.join("Evil.toc"), "## Interface: 16001\n").unwrap();
        // 64-bit Mach-O header behind a harmless-looking extension.
        fs::write(addon.join("data.bin"), [0xCF, 0xFA, 0xED, 0xFE, 0x07, 0x00]).unwrap();
        let err = safety_scan(&root).unwrap_err().to_string();
        assert!(err.contains("macOS executable"), "{err}");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn safety_scan_refuses_other_runnable_file_types() {
        let root = std::env::temp_dir().join(format!("af-types-{}", unique()));
        let addon = root.join("Evil");
        fs::create_dir_all(&addon).unwrap();
        fs::write(addon.join("Evil.toc"), "## Interface: 16001\n").unwrap();
        fs::write(addon.join("notes.lua"), "-- fine\n").unwrap();
        assert!(safety_scan(&root).is_ok());
        for name in ["run.pyw", "open.url", "help.chm", "console.msc", "pack.msix", "disk.iso", "SHOUT.MJS"] {
            let file = addon.join(name);
            fs::write(&file, "text, no magic bytes").unwrap();
            let err = safety_scan(&root).unwrap_err().to_string();
            assert!(err.contains("executable file"), "{name}: {err}");
            fs::remove_file(&file).unwrap();
        }
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn safety_scan_refuses_shebang_and_app_bundle() {
        let root = std::env::temp_dir().join(format!("af-mac-{}", unique()));
        let addon = root.join("Evil");
        fs::create_dir_all(&addon).unwrap();
        fs::write(addon.join("Evil.toc"), "## Interface: 16001\n").unwrap();
        fs::write(addon.join("run"), "#!/bin/sh\necho hi\n").unwrap();
        let err = safety_scan(&root).unwrap_err().to_string();
        assert!(err.contains("script in disguise"), "{err}");
        fs::remove_file(addon.join("run")).unwrap();
        fs::create_dir_all(addon.join("Helper.app").join("Contents")).unwrap();
        let err = safety_scan(&root).unwrap_err().to_string();
        assert!(err.contains("application bundle"), "{err}");
        let _ = fs::remove_dir_all(&root);
    }
}

#[cfg(test)]
mod recovery_tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("af-recovery-{name}-{}", unique()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn interrupted_install_puts_back_the_old_folder() {
        let addons = scratch("restore");
        // A crash after the old folder was moved aside and before the new one landed.
        let old = addons.join(TMP_DIR).join("work1").join("old").join("Questie");
        fs::create_dir_all(&old).unwrap();
        fs::write(old.join("Questie.lua"), "keep me").unwrap();
        clean_tmp(&addons);
        assert_eq!(fs::read_to_string(addons.join("Questie").join("Questie.lua")).unwrap(), "keep me");
        assert!(!addons.join(TMP_DIR).join("work1").exists());
    }

    #[test]
    fn interrupted_install_keeps_old_folder_when_a_new_one_exists() {
        let addons = scratch("keep");
        fs::create_dir_all(addons.join("Questie")).unwrap();
        fs::write(addons.join("Questie").join("new.lua"), "new").unwrap();
        let old = addons.join(TMP_DIR).join("work2").join("old").join("Questie");
        fs::create_dir_all(&old).unwrap();
        fs::write(old.join("old.lua"), "old").unwrap();
        clean_tmp(&addons);
        // The new install stays in place, and the old folder is kept for recovery.
        assert!(addons.join("Questie").join("new.lua").exists());
        assert!(addons.join(TMP_DIR).join("recovered").join("work2-Questie").join("old.lua").exists());
    }

    #[test]
    fn extraction_refuses_paths_that_leave_the_folder() {
        let dir = scratch("zip");
        let zip_path = dir.join("evil.zip");
        {
            let file = fs::File::create(&zip_path).unwrap();
            let mut w = zip::ZipWriter::new(file);
            let opts = zip::write::SimpleFileOptions::default();
            w.start_file("../escape.txt", opts).unwrap();
            std::io::Write::write_all(&mut w, b"nope").unwrap();
            w.finish().unwrap();
        }
        let mut archive = zip::ZipArchive::new(fs::File::open(&zip_path).unwrap()).unwrap();
        let out = dir.join("out");
        fs::create_dir_all(&out).unwrap();
        assert!(extract_limited(&mut archive, &out).is_err());
        assert!(!dir.join("escape.txt").exists());
    }

    /// A zip built entry by entry, for archives no packager would make.
    fn raw_zip(dir: &Path, build: impl FnOnce(&mut zip::ZipWriter<fs::File>)) -> zip::ZipArchive<fs::File> {
        let path = dir.join(format!("{}.zip", unique()));
        let mut w = zip::ZipWriter::new(fs::File::create(&path).unwrap());
        build(&mut w);
        w.finish().unwrap();
        zip::ZipArchive::new(fs::File::open(&path).unwrap()).unwrap()
    }

    fn file(w: &mut zip::ZipWriter<fs::File>, name: &str, body: &[u8]) {
        let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        w.start_file(name, opts).unwrap();
        std::io::Write::write_all(w, body).unwrap();
    }

    #[test]
    fn names_windows_would_store_differently_are_refused() {
        for name in [
            "Foo/notes.lua:payload.exe", // alternate data stream: invisible to the safety scan
            "Foo/CON",
            "Foo/nul.lua",
            "Foo/Com1.txt",
            "Foo/LPT9",
            "Foo/trailing.",
            "Foo/trailing ",
            "Foo/Sub./x.lua",
            "Foo/what?.lua",
            "Foo/tab\there.lua",
        ] {
            let dir = scratch("names");
            let mut archive = raw_zip(&dir, |w| {
                file(w, "Foo/Foo.toc", b"## Interface: 16001\n");
                file(w, name, b"x");
            });
            let out = dir.join("out");
            let err = extract_limited(&mut archive, &out).unwrap_err().to_string();
            assert!(err.contains("cannot store"), "{name:?}: {err}");
            let _ = fs::remove_dir_all(&dir);
        }
    }

    #[test]
    fn ordinary_addon_names_are_not_mistaken_for_devices() {
        for name in ["Console.lua", "aux-addon", "COM10.lua", "Auxiliary.tga", "Nullify.lua", "LPT.lua", "con_fig.xml", "v1.2.3"] {
            assert!(!unsafe_name(name), "{name}");
        }
        for name in ["", "AUX", "aux.lua", "CONOUT$", "x:y", "x.", "x "] {
            assert!(unsafe_name(name), "{name:?}");
        }
    }

    #[test]
    fn an_archive_that_expands_past_the_limit_stops_at_the_limit() {
        let dir = scratch("bomb");
        // 256 KB of zeros deflates to a few hundred bytes.
        let mut archive = raw_zip(&dir, |w| file(w, "Foo/zeros.lua", &vec![0u8; 256 * 1024]));
        let out = dir.join("out");
        let limits = ExtractLimits { entries: 10, bytes: 16 * 1024 };
        let err = extract_with(&mut archive, &out, &limits).unwrap_err().to_string();
        assert!(err.contains("limit"), "{err}");
        // It stopped writing one byte past the limit, not at the end of the entry.
        let written = fs::metadata(out.join("Foo").join("zeros.lua")).unwrap().len();
        assert!(written <= limits.bytes + 1, "wrote {written} bytes");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_archive_with_too_many_entries_is_refused_before_extracting() {
        let dir = scratch("entries");
        let mut archive = raw_zip(&dir, |w| {
            for i in 0..5 {
                file(w, &format!("Foo/{i}.lua"), b"x");
            }
        });
        let out = dir.join("out");
        let err = extract_with(&mut archive, &out, &ExtractLimits { entries: 4, bytes: 1 << 20 }).unwrap_err().to_string();
        assert!(err.contains("more than the 4 allowed"), "{err}");
        assert!(!out.join("Foo").exists(), "nothing is written before the count is checked");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_symlink_entry_is_written_as_a_plain_file() {
        let dir = scratch("symlink");
        let mut archive = raw_zip(&dir, |w| {
            file(w, "Foo/Foo.toc", b"## Interface: 16001\n");
            w.add_symlink("Foo/link", "../../../outside", zip::write::SimpleFileOptions::default()).unwrap();
        });
        let out = dir.join("out");
        extract_limited(&mut archive, &out).unwrap();
        let link = out.join("Foo").join("link");
        let meta = fs::symlink_metadata(&link).unwrap();
        assert!(meta.is_file() && !meta.file_type().is_symlink(), "a link entry must not become a link");
        assert_eq!(fs::read_to_string(&link).unwrap(), "../../../outside");
        let _ = fs::remove_dir_all(&dir);
    }

    fn addons_with_existing(name: &str) -> (PathBuf, PathBuf) {
        let root = scratch("place");
        let addons = root.join("AddOns");
        fs::create_dir_all(addons.join(name)).unwrap();
        fs::write(addons.join(name).join("keep.lua"), "the user's copy").unwrap();
        (root, addons)
    }

    fn zip_file(root: &Path, entries: &[(&str, &str)]) -> PathBuf {
        let path = root.join(format!("{}.zip", unique()));
        let mut w = zip::ZipWriter::new(fs::File::create(&path).unwrap());
        for (name, body) in entries {
            file(&mut w, name, body.as_bytes());
        }
        w.finish().unwrap();
        path
    }

    #[test]
    fn a_toc_with_no_name_cannot_point_the_install_at_addons_itself() {
        let (root, addons) = addons_with_existing("Questie");
        let zip = zip_file(&root, &[("Wrap-1.0/.toc", "## Interface: 16001\n"), ("Wrap-1.0/x.lua", "x")]);
        let err = place(&zip, &addons, &HashMap::new()).unwrap_err().to_string();
        assert!(err.contains("suspicious folder name"), "{err}");
        assert_eq!(fs::read_to_string(addons.join("Questie").join("keep.lua")).unwrap(), "the user's copy");
        assert!(!addons.join("x.lua").exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn two_folders_with_one_target_name_are_refused() {
        let (root, addons) = addons_with_existing("Shared");
        let zip = zip_file(&root, &[
            ("Shared-1.0/Shared.toc", "## Version: 1\n"),
            ("Shared-2.0/Shared_Forever.toc", "## Version: 2\n"),
        ]);
        let err = place(&zip, &addons, &HashMap::new()).unwrap_err().to_string();
        assert!(err.contains("two addon folders named Shared"), "{err}");
        assert_eq!(fs::read_to_string(addons.join("Shared").join("keep.lua")).unwrap(), "the user's copy");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn folders_that_differ_only_in_case_never_install_twice() {
        let (root, addons) = addons_with_existing("Other");
        let zip = zip_file(&root, &[("Foo/Foo.toc", "## Version: 1\n"), ("foo/foo.toc", "## Version: 2\n")]);
        // Windows merges the two while extracting (one folder); other systems keep two and refuse.
        match place(&zip, &addons, &HashMap::new()) {
            Ok(installed) => assert_eq!(installed.len(), 1, "{installed:?}"),
            Err(e) => assert!(e.to_string().contains("two addon folders"), "{e}"),
        }
        let foos = fs::read_dir(&addons).unwrap().flatten().filter(|e| e.file_name().eq_ignore_ascii_case("foo")).count();
        assert!(foos <= 1, "{foos} folders named foo");
        let _ = fs::remove_dir_all(&root);
    }

    /// Serve one HTTP reply that promises more bytes than it sends, then hangs up.
    fn serve_truncated() -> String {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            let (mut sock, _) = listener.accept().unwrap();
            let mut buf = [0u8; 1024];
            let _ = sock.read(&mut buf);
            let _ = sock.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 100000\r\nConnection: close\r\n\r\nPK\x03\x04 only the start");
        });
        format!("http://{addr}/Addon.zip")
    }

    #[test]
    fn a_cut_off_download_leaves_no_file_behind() {
        let dir = scratch("download");
        let staged = dir.join("downloads");
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let url = serve_truncated();
        let net = crate::net::Net::for_tests(&["127.0.0.1"]);
        let result = rt.block_on(download(&net, crate::net::Purpose::GitHub, &url, &staged, "Addon.zip", &[]));
        assert!(result.is_err(), "a truncated body must fail");
        assert_eq!(fs::read_dir(&staged).unwrap().count(), 0, "the partial file must be removed");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_download_the_gate_refuses_says_why_and_writes_nothing() {
        let dir = scratch("refused");
        let staged = dir.join("downloads");
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let net = crate::net::Net::for_tests(&["127.0.0.1"]);
        net.set_policy(crate::net::Policy { offline: true, ..Default::default() });
        // Nothing listens on port 1; a request that went out would fail differently.
        let url = "http://127.0.0.1:1/Addon.zip";
        let err = rt.block_on(download(&net, crate::net::Purpose::GitHub, url, &staged, "Addon.zip", &[])).unwrap_err();
        assert!(err.is::<crate::net::Refused>(), "{err:#}");
        assert!(err.to_string().starts_with("Offline mode is on"), "{err}");
        assert_eq!(fs::read_dir(&staged).unwrap().count(), 0);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn abandoned_downloads_are_cleared_and_nothing_else() {
        let dir = scratch("stale");
        fs::write(dir.join("abc-Addon.zip"), "old").unwrap();
        fs::write(dir.join("notes.txt"), "not ours").unwrap();
        fs::create_dir_all(dir.join("folder.zip")).unwrap();
        // Fresh files survive the startup sweep.
        assert_eq!(clean_downloads(&dir), 0);
        assert!(dir.join("abc-Addon.zip").exists());
        // Once old enough, only the plain zip goes.
        assert_eq!(clean_downloads_older_than(&dir, std::time::Duration::ZERO), 1);
        assert!(!dir.join("abc-Addon.zip").exists());
        assert!(dir.join("notes.txt").exists());
        assert!(dir.join("folder.zip").is_dir());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_zip_from_disk_is_installed_from_a_copy_and_the_users_file_is_kept() {
        let root = scratch("local");
        let addons = root.join("AddOns");
        let downloads = root.join("Downloads");
        fs::create_dir_all(&downloads).unwrap();
        let original = downloads.join("Nether Suite (1.2).zip");
        fs::copy(zip_file(&root, &[("NetherSuite-1.2/NetherSuite.toc", "## Version: 1.2\n"), ("NetherSuite-1.2/core.lua", "x")]), &original).unwrap();
        // Downloaded a week ago: older than the startup sweep's limit.
        let week_ago = SystemTime::now() - std::time::Duration::from_secs(7 * 24 * 3600);
        fs::File::options().write(true).open(&original).unwrap().set_modified(week_ago).unwrap();
        let before = fs::read(&original).unwrap();
        let staging = root.join("staging");
        let staged = stage_local(&original, &staging).unwrap();
        assert!(staged.starts_with(&staging));
        let staged_name = staged.file_name().unwrap().to_string_lossy().to_string();
        assert!(staged_name.ends_with("-Nether_Suite__1.2_.zip"), "{staged_name}");
        assert_eq!(fs::read(&staged).unwrap(), before);
        // A fresh file: the startup sweep must not take it for an old download.
        assert!(fs::metadata(&staged).unwrap().modified().unwrap().elapsed().unwrap() < std::time::Duration::from_secs(60));
        assert_eq!(place(&staged, &addons, &HashMap::new()).unwrap(), vec!["NetherSuite"]);
        assert!(addons.join("NetherSuite").join("core.lua").exists());
        assert!(!staged.exists(), "the staged copy goes once it is installed");
        assert_eq!(fs::read(&original).unwrap(), before, "the user's own zip is never changed or removed");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn only_a_zip_file_is_staged() {
        let root = scratch("local-refused");
        let staging = root.join("staging");
        let left_behind = || fs::read_dir(&staging).map(|d| d.count()).unwrap_or(0);
        fs::write(root.join("notes.txt"), "not an archive").unwrap();
        let err = stage_local(&root.join("notes.txt"), &staging).unwrap_err().to_string();
        assert!(err.contains("choose a .zip file"), "{err}");
        fs::create_dir_all(root.join("folder.zip")).unwrap();
        assert!(stage_local(&root.join("folder.zip"), &staging).unwrap_err().to_string().contains("not a file"));
        assert!(stage_local(&root.join("missing.zip"), &staging).is_err());
        fs::write(root.join("big.zip"), vec![0u8; 64]).unwrap();
        let err = stage_local_capped(&root.join("big.zip"), &staging, 63).unwrap_err().to_string();
        assert!(err.contains("larger than"), "{err}");
        assert_eq!(left_behind(), 0, "nothing is left in the staging folder");
        assert!(stage_local_capped(&root.join("big.zip"), &staging, 64).is_ok(), "exactly the cap is allowed");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn the_plan_sees_only_archives_that_passed_every_check_and_can_still_stop_the_install() {
        let (root, addons) = addons_with_existing("Core");
        // A plan that refuses: nothing in AddOns changes, and the staged zip is gone.
        let zip = zip_file(&root, &[("Core-2.0/Core_Forever.toc", "## Version: 2\n"), ("Core-2.0/new.lua", "new")]);
        let mut seen = Vec::new();
        let err = place_planned::<()>(&zip, &addons, |found| {
            seen = found.iter().map(|(_, name)| name.clone()).collect::<Vec<_>>();
            Err(anyhow!("NOT_FOREVER"))
        })
        .unwrap_err();
        assert_eq!(err.to_string(), "NOT_FOREVER");
        assert_eq!(seen, vec!["Core"], "the plan sees the name the folder installs under");
        assert_eq!(fs::read_to_string(addons.join("Core").join("keep.lua")).unwrap(), "the user's copy");
        assert!(!zip.exists());
        assert!(!addons.join(TMP_DIR).exists(), "the work folder is cleared");
        // An archive that fails a check never reaches the plan.
        let mut called = false;
        let zip = zip_file(&root, &[("Core/Core.toc", "## Version: 3\n"), ("Core/helper.exe", "MZ")]);
        let err = place_planned(&zip, &addons, |_| {
            called = true;
            Ok((HashMap::new(), ()))
        })
        .unwrap_err()
        .to_string();
        assert!(err.contains("SAFETY"), "{err}");
        assert!(!called);
        assert_eq!(fs::read_to_string(addons.join("Core").join("keep.lua")).unwrap(), "the user's copy");
        // The owners the plan returns are enforced like any others.
        let zip = zip_file(&root, &[("Core/Core.toc", "## Version: 3\n")]);
        let err = place_planned(&zip, &addons, |_| Ok((HashMap::from([("core".to_string(), "github:a/Core".to_string())]), ())))
            .unwrap_err()
            .to_string();
        assert!(err.contains("cannot replace addon folder Core: it belongs to github:a/Core"), "{err}");
        assert_eq!(fs::read_to_string(addons.join("Core").join("keep.lua")).unwrap(), "the user's copy");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn recovery_waits_while_another_copy_holds_the_lock() {
        let addons = scratch("lock");
        let old = addons.join(TMP_DIR).join("work3").join("old").join("Atlas");
        fs::create_dir_all(&old).unwrap();
        fs::write(old.join("Atlas.lua"), "mid install").unwrap();
        let _held = lock_tmp(&addons, true).unwrap();
        clean_tmp(&addons);
        // Nothing was moved: the other copy is still installing.
        assert!(old.join("Atlas.lua").exists());
        assert!(!addons.join("Atlas").exists());
    }

    #[test]
    fn removal_waits_for_the_install_lock() {
        let addons = scratch("remove-lock");
        let addon = addons.join("Atlas");
        fs::create_dir_all(&addon).unwrap();
        fs::write(addon.join("Atlas.lua"), "x").unwrap();
        // Another copy is installing: it holds the lock.
        let held = lock_tmp(&addons, true).unwrap();
        let (dir, folders) = (addons.clone(), vec!["Atlas".to_string()]);
        let worker = std::thread::spawn(move || remove_folders(&dir, &folders));
        std::thread::sleep(std::time::Duration::from_millis(300));
        // The removal must still be waiting, not deleting under the install.
        assert!(addon.exists());
        drop(held);
        let gone = worker.join().unwrap().unwrap();
        assert_eq!(gone, vec!["Atlas".to_string()]);
        assert!(!addon.exists());
    }
}

