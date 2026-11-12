//! Secrets (API keys) live in the operating system's credential store:
//! Windows Credential Manager, or the macOS Keychain. Never in `state.json`,
//! the log, a diagnostics report or an exported addon list.
//!
//! The store encrypts each secret for the signed-in OS user. That keeps it out
//! of files other programs and backups read; it does not hide it from other
//! programs running as the same user, and nothing here claims that it does.

use anyhow::{anyhow, Result};
use keyring_core::{CredentialStore, Error};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// The "service" every AddonForge secret is filed under.
const SERVICE: &str = "AddonForge";

/// Name of the user's own CurseForge API key.
pub const CURSEFORGE_KEY: &str = "curseforge-api-key";

pub struct Secrets {
    store: Arc<CredentialStore>,
    label: &'static str,
    /// Stays on this PC: a Windows credential is not copied to other domain machines.
    local_only: bool,
    /// The stores do not promise to order two calls on one entry made from
    /// different threads, so every call goes through here one at a time.
    gate: Mutex<()>,
}

impl Secrets {
    /// The credential store of the OS this build runs on.
    pub fn os() -> Result<Self> {
        #[cfg(windows)]
        {
            let store = windows_native_keyring_store::Store::new().map_err(|e| anyhow!("Windows Credential Manager: {e}"))?;
            Ok(Self { store, label: "Windows Credential Manager", local_only: true, gate: Mutex::new(()) })
        }
        #[cfg(target_os = "macos")]
        {
            let store = apple_native_keyring_store::keychain::Store::new().map_err(|e| anyhow!("macOS Keychain: {e}"))?;
            Ok(Self { store, label: "the macOS Keychain", local_only: false, gate: Mutex::new(()) })
        }
        #[cfg(not(any(windows, target_os = "macos")))]
        {
            Err(anyhow!("this platform has no supported credential store yet"))
        }
    }

    /// A store that lives and dies with the process, for tests.
    #[cfg(test)]
    pub fn in_memory() -> Self {
        let store = keyring_core::mock::Store::new().expect("mock store");
        Self { store, label: "memory", local_only: false, gate: Mutex::new(()) }
    }

    /// What to call the store when telling the user where a key is kept.
    pub fn label(&self) -> &'static str {
        self.label
    }

    fn entry(&self, name: &str) -> Result<keyring_core::Entry> {
        let local: HashMap<&str, &str> = HashMap::from([("persistence", "Local")]);
        let modifiers = self.local_only.then_some(&local);
        self.store.build(SERVICE, name, modifiers).map_err(|e| anyhow!("{}: {e}", self.label))
    }

    /// The secret stored under `name`, or `None` when there isn't one.
    pub fn get(&self, name: &str) -> Result<Option<String>> {
        let _one_at_a_time = self.gate.lock().unwrap_or_else(|p| p.into_inner());
        match self.entry(name)?.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(Error::NoEntry) => Ok(None),
            Err(e) => Err(anyhow!("{}: {e}", self.label)),
        }
    }

    pub fn set(&self, name: &str, value: &str) -> Result<()> {
        let _one_at_a_time = self.gate.lock().unwrap_or_else(|p| p.into_inner());
        self.entry(name)?.set_password(value).map_err(|e| anyhow!("{}: {e}", self.label))
    }

    /// Remove the secret. Removing one that isn't there is not an error.
    pub fn delete(&self, name: &str) -> Result<()> {
        let _one_at_a_time = self.gate.lock().unwrap_or_else(|p| p.into_inner());
        match self.entry(name)?.delete_credential() {
            Ok(()) | Err(Error::NoEntry) => Ok(()),
            Err(e) => Err(anyhow!("{}: {e}", self.label)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_secret_round_trips_and_can_be_removed() {
        let secrets = Secrets::in_memory();
        assert_eq!(secrets.get(CURSEFORGE_KEY).unwrap(), None, "nothing stored yet");
        secrets.set(CURSEFORGE_KEY, "first").unwrap();
        secrets.set(CURSEFORGE_KEY, "second").unwrap();
        assert_eq!(secrets.get(CURSEFORGE_KEY).unwrap().as_deref(), Some("second"), "saving again replaces");
        assert_eq!(secrets.get("something-else").unwrap(), None, "names do not share a value");
        secrets.delete(CURSEFORGE_KEY).unwrap();
        assert_eq!(secrets.get(CURSEFORGE_KEY).unwrap(), None);
        secrets.delete(CURSEFORGE_KEY).unwrap(); // already gone: still fine
    }

    /// Talks to the real credential store of this machine, under a name no
    /// real key uses, and removes it again. Run on purpose:
    /// `cargo test -- --ignored real_credential_store`.
    #[test]
    #[ignore = "writes to and removes from the real OS credential store"]
    fn real_credential_store_round_trips() {
        let secrets = Secrets::os().expect("credential store");
        let name = format!("selftest-{}", std::process::id());
        secrets.delete(&name).unwrap();
        assert_eq!(secrets.get(&name).unwrap(), None);
        secrets.set(&name, "$2a$10$not.a.real.key").unwrap();
        assert_eq!(secrets.get(&name).unwrap().as_deref(), Some("$2a$10$not.a.real.key"));
        secrets.delete(&name).unwrap();
        assert_eq!(secrets.get(&name).unwrap(), None, "removed again");
    }
}
