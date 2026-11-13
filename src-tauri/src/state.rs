//! Persistent local state: `%APPDATA%\AddonForge\state.json` (macOS: `~/Library/Application Support/AddonForge`).
//!
//! This is the only settings file the app writes outside the game's AddOns
//! folder (plus `addonforge.log` beside it). Written atomically (tmp +
//! rename) so a crash can never corrupt it.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Installed {
    /// Source kind: github | wago | wowi | tukui | curse, or `local` for a zip
    /// the user installed from disk (never updated by the app).
    pub source: String,
    /// The source's id; for `local`, the zip's file name.
    pub source_id: String,
    pub version: String,
    pub folders: Vec<String>,
    pub installed_at: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct AppState {
    /// Product folder we manage, e.g. `C:\...\World of Warcraft\_classic_beta_`.
    #[serde(default)]
    pub install_path: Option<String>,
    // `wago_key` (a Wago Addons API key from before build 25) is gone: unused
    // since Wago's updater keys went Patreon-only. An older file that still has
    // one loads as before (unknown fields are ignored), and the next save
    // leaves the key out, so it does not stay on disk in plain text.
    /// Packages we installed ourselves, keyed by package key.
    #[serde(default)]
    pub installed: HashMap<String, Installed>,
    /// Consider GitHub pre-releases and Wago beta builds when checking.
    #[serde(default)]
    pub allow_prerelease: bool,
    /// Package keys whose updates are held back.
    #[serde(default)]
    pub pinned: BTreeSet<String>,
    /// Package keys hidden from the Installed list.
    #[serde(default)]
    pub ignored: BTreeSet<String>,
    /// A self-update version the user chose to skip ("v0.2.0-b7").
    #[serde(default)]
    pub skip_self_update: Option<String>,
    /// Build number of the last version that ran, for the one-time "what's new" note.
    #[serde(default)]
    pub last_seen_build: u32,
    /// Install every available update as soon as the launch check finishes
    /// (pinned and ignored addons are left alone).
    #[serde(default)]
    pub auto_update: bool,
    /// A live `_forever_` folder the user chose not to switch to from the beta
    /// folder, so the hint is not shown again for that path.
    #[serde(default)]
    pub dismissed_live_hint: Option<String>,
    /// Offline mode and the network switches (see `net::Policy`). Missing in
    /// older files, which then keep every switch on.
    #[serde(default)]
    pub network: crate::net::Policy,
}

pub fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

impl AppState {
    /// `%APPDATA%\AddonForge`. `ADDONFORGE_STATE_DIR` overrides it so tests and
    /// screenshots can use a throwaway profile without touching the real one.
    pub fn dir() -> PathBuf {
        if let Some(dir) = std::env::var_os("ADDONFORGE_STATE_DIR").filter(|v| !v.is_empty()) {
            return PathBuf::from(dir);
        }
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("AddonForge")
    }

    pub fn path() -> PathBuf {
        Self::dir().join("state.json")
    }

    pub fn load() -> Self {
        Self::load_from(&Self::dir())
    }

    /// Current settings, else the last good copy, else empty.
    pub fn load_from(dir: &Path) -> Self {
        let read = |p: PathBuf| -> Option<Self> { serde_json::from_slice(&fs::read(p).ok()?).ok() };
        read(dir.join("state.json")).or_else(|| read(dir.join("state.json.bak"))).unwrap_or_default()
    }

    /// Exclusive lock on the settings folder, shared by every copy of the app.
    /// Held across reload -> change -> save so no copy can overwrite another's change.
    pub fn lock_settings() -> anyhow::Result<fs::File> {
        let dir = Self::dir();
        fs::create_dir_all(&dir)?;
        let file = fs::OpenOptions::new().create(true).write(true).truncate(false).open(dir.join("state.lock"))?;
        file.lock()?;
        Ok(file)
    }

    pub fn save(&self) -> anyhow::Result<()> {
        self.save_to(&Self::dir())
    }

    /// Write the settings atomically. The last good copy is kept as `state.json.bak`:
    /// it is only replaced by a current file that still reads back as valid, so a
    /// corrupt file can never overwrite the only good copy.
    pub fn save_to(&self, dir: &Path) -> anyhow::Result<()> {
        fs::create_dir_all(dir)?;
        let tmp = dir.join("state.json.tmp");
        fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        let final_path = dir.join("state.json");
        // The backup is the current file as this version reads it, written out
        // again: a field this version no longer has (an old `wago_key`) does not
        // live on in the backup either.
        let current = fs::read(&final_path).ok().and_then(|b| serde_json::from_slice::<AppState>(&b).ok());
        if let Some(previous) = current {
            if let Ok(bytes) = serde_json::to_vec_pretty(&previous) {
                let _ = fs::write(dir.join("state.json.bak"), bytes);
            }
        }
        // Rust's rename replaces an existing file on Windows, so the state file is
        // never missing part-way through a save.
        fs::rename(&tmp, &final_path)?;
        Ok(())
    }
}

#[cfg(test)]
mod recovery_tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("af-state-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_corrupt_settings_file_does_not_overwrite_the_good_backup() {
        let dir = scratch("keep-backup");
        let good = AppState { allow_prerelease: true, ..AppState::default() };
        good.save_to(&dir).unwrap();
        good.save_to(&dir).unwrap(); // the good copy is now also the backup
        fs::write(dir.join("state.json"), "{ not json").unwrap();
        // Saving over a corrupt file must leave the good backup in place.
        AppState::default().save_to(&dir).unwrap();
        let backup: AppState = serde_json::from_slice(&fs::read(dir.join("state.json.bak")).unwrap()).unwrap();
        assert!(backup.allow_prerelease);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_old_wago_key_loads_and_is_left_out_of_the_next_save() {
        let dir = scratch("wago-key");
        // A placeholder, not key-shaped: a secret scanner has nothing to flag.
        let old = r#"{"install_path":"C:\\Games\\_forever_","wago_key":"old","allow_prerelease":true,
            "installed":{"cat:x":{"source":"github","source_id":"a/b","version":"1","folders":["X"],"installed_at":1}}}"#;
        fs::write(dir.join("state.json"), old).unwrap();
        let st = AppState::load_from(&dir);
        assert_eq!(st.install_path.as_deref(), Some(r"C:\Games\_forever_"), "the rest of an old file still loads");
        assert!(st.allow_prerelease && st.installed.contains_key("cat:x"));
        st.save_to(&dir).unwrap();
        for file in ["state.json", "state.json.bak"] {
            let saved = fs::read_to_string(dir.join(file)).unwrap();
            assert!(!saved.contains("wago"), "{file}: {saved}");
            assert!(saved.contains("_forever_"), "{file} still holds the settings");
        }
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_falls_back_to_the_backup_when_the_main_file_is_corrupt() {
        let dir = scratch("fallback");
        let good = AppState { auto_update: true, ..AppState::default() };
        good.save_to(&dir).unwrap();
        good.save_to(&dir).unwrap();
        fs::write(dir.join("state.json"), "broken").unwrap();
        assert!(AppState::load_from(&dir).auto_update);
        let _ = fs::remove_dir_all(&dir);
    }
}

