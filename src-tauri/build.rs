use std::env;
use std::fs;
use std::path::Path;

/// An optional built-in CurseForge 3rd Party API key. It is read from
/// `private/curseforge-key.txt` (gitignored) or the `ADDONFORGE_CURSEFORGE_KEY`
/// environment variable at compile time and written as a plain string constant,
/// never anywhere else.
///
/// Only bake one in if your agreement with CurseForge covers shipping it:
/// their terms issue a key to one developer and forbid passing it on, and a
/// key inside a published exe can be read back by anyone. Builds without one
/// (the default, CI, contributors) use the key each user adds in Settings,
/// which is kept in the OS credential store. See docs/PROVIDER_INTEGRATIONS.md.
fn curseforge_key() -> Option<String> {
    println!("cargo:rerun-if-env-changed=ADDONFORGE_CURSEFORGE_KEY");
    let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("private").join("curseforge-key.txt");
    println!("cargo:rerun-if-changed={}", file.display());
    let from_env = env::var("ADDONFORGE_CURSEFORGE_KEY").ok();
    let from_file = fs::read_to_string(&file).ok();
    from_env
        .or(from_file)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn main() {
    // Refresh the embedded Windows resources when only the artwork changes.
    println!("cargo:rerun-if-changed=icons/icon.ico");
    println!("cargo:rerun-if-changed=icons/icon.png");

    // Plain embedding since build 26: the old XOR mask hid nothing (any copy of
    // the app can be read back) and its byte pattern was a detector signal.
    let key = curseforge_key().unwrap_or_default();
    let out = Path::new(&env::var("OUT_DIR").expect("OUT_DIR")).join("cf_key.rs");
    let body = format!("pub const CF_KEY: &str = {key:?};\n");
    fs::write(&out, body).expect("write cf_key.rs");
    println!(
        "cargo:warning=CurseForge key: {}",
        if key.is_empty() { "none built in (users add their own in Settings)" } else { "embedded" }
    );

    tauri_build::build()
}
