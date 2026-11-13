//! Addon sources. Each resolves "what is the latest release and where is
//! the zip" for one addon. The app talks to these sites directly over
//! HTTPS; nothing goes through a server of ours.

pub mod curseforge;
pub mod github;
pub mod tukui;
pub mod wago;
pub mod wowi;

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Hash)]
#[serde(tag = "kind", content = "id", rename_all = "lowercase")]
pub enum Source {
    Github(String),
    Wago(String),
    Wowi(u64),
    Tukui(String),
    /// CurseForge project id (only usable in builds that carry the API key).
    Curse(u64),
}

impl Source {
    pub fn kind(&self) -> &'static str {
        match self {
            Source::Github(_) => "github",
            Source::Wago(_) => "wago",
            Source::Wowi(_) => "wowi",
            Source::Tukui(_) => "tukui",
            Source::Curse(_) => "curse",
        }
    }
    pub fn id(&self) -> String {
        match self {
            Source::Github(r) => r.clone(),
            Source::Wago(i) => i.clone(),
            Source::Wowi(i) => i.to_string(),
            Source::Tukui(s) => s.clone(),
            Source::Curse(i) => i.to_string(),
        }
    }
    pub fn label(&self) -> String {
        match self {
            Source::Github(r) => format!("GitHub · {r}"),
            Source::Wago(_) => "Wago".into(),
            Source::Wowi(_) => "WoWInterface".into(),
            Source::Tukui(_) => "TukUI".into(),
            Source::Curse(_) => "CurseForge".into(),
        }
    }
    /// What the network gate calls requests to this source.
    pub fn purpose(&self) -> crate::net::Purpose {
        use crate::net::Purpose;
        match self {
            Source::Github(_) => Purpose::GitHub,
            Source::Wago(_) => Purpose::Wago,
            Source::Wowi(_) => Purpose::WoWInterface,
            Source::Tukui(_) => Purpose::Tukui,
            Source::Curse(_) => Purpose::CurseForge,
        }
    }
    pub fn url(&self) -> String {
        match self {
            Source::Github(r) => format!("https://github.com/{r}/releases/latest"),
            Source::Wago(i) => format!("https://addons.wago.io/addons/{i}"),
            Source::Wowi(i) => format!("https://www.wowinterface.com/downloads/info{i}"),
            Source::Tukui(s) => format!("https://tukui.org/{s}"),
            Source::Curse(i) => curseforge::project_url(*i),
        }
    }
    /// Rebuild from the strings stored in state.json.
    pub fn from_parts(kind: &str, id: &str) -> Option<Source> {
        match kind {
            "github" => Some(Source::Github(id.to_string())),
            "wago" => Some(Source::Wago(id.to_string())),
            "wowi" => id.parse().ok().map(Source::Wowi),
            "tukui" => Some(Source::Tukui(id.to_string())),
            "curse" => id.parse().ok().map(Source::Curse),
            _ => None,
        }
    }
}

/// The latest release as the source reports it.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Remote {
    pub version: String,
    pub download_url: String,
    pub filename: String,
    /// Some(true) = source says this build supports Forever,
    /// Some(false) = it says it doesn't, None = the source can't tell us.
    pub forever: Option<bool>,
    /// True when this came from a pre-release / beta channel.
    pub prerelease: bool,
    /// The hash the source publishes for this exact file, when it publishes
    /// one; the download is checked against it (`install::download`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hash: Option<FileHash>,
}

/// A hash a source publishes for the file it serves. Lowercase hex.
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(tag = "algo", content = "hex", rename_all = "lowercase")]
pub enum FileHash {
    /// GitHub release assets (`digest`).
    Sha256(String),
    /// CurseForge files (`hashes`, algo 1).
    Sha1(String),
    /// WoWInterface (`UIMD5`), and CurseForge files with no SHA-1 (algo 2).
    Md5(String),
}

impl FileHash {
    /// Hex of exactly the right length for its kind, or nothing: a value a
    /// source sent in some other shape is not trusted to mean anything.
    fn checked(hex: &str, len: usize) -> Option<String> {
        let hex = hex.trim().to_ascii_lowercase();
        (hex.len() == len && hex.bytes().all(|b| b.is_ascii_hexdigit())).then_some(hex)
    }

    pub fn sha256(hex: &str) -> Option<Self> {
        Self::checked(hex, 64).map(Self::Sha256)
    }

    pub fn sha1(hex: &str) -> Option<Self> {
        Self::checked(hex, 40).map(Self::Sha1)
    }

    pub fn md5(hex: &str) -> Option<Self> {
        Self::checked(hex, 32).map(Self::Md5)
    }

    /// GitHub's asset `digest`: `sha256:<hex>`. Other algorithms are not used.
    pub fn from_github_digest(digest: &str) -> Option<Self> {
        digest.trim().strip_prefix("sha256:").and_then(Self::sha256)
    }

    pub fn label(&self) -> &'static str {
        match self {
            FileHash::Sha256(_) => "SHA-256",
            FileHash::Sha1(_) => "SHA-1",
            FileHash::Md5(_) => "MD5",
        }
    }

    pub fn hex(&self) -> &str {
        match self {
            FileHash::Sha256(h) | FileHash::Sha1(h) | FileHash::Md5(h) => h,
        }
    }
}

/// Options that can influence resolution.
#[derive(Default, Clone, Debug)]
pub struct ResolveOpts<'a> {
    pub asset_hint: Option<&'a str>,
    pub prerelease: bool,
}

pub async fn resolve(
    net: &crate::net::Net,
    src: &Source,
    opts: ResolveOpts<'_>,
) -> anyhow::Result<Remote> {
    match src {
        Source::Github(repo) => github::resolve(net, repo, opts.asset_hint, opts.prerelease).await,
        // Dormant: Wago's updater keys went Patreon-only, and no key is kept.
        Source::Wago(id) => wago::resolve(net, id, None, opts.prerelease).await,
        Source::Wowi(id) => wowi::resolve(net, *id).await,
        Source::Tukui(slug) => tukui::resolve(net, slug).await,
        Source::Curse(id) => curseforge::resolve(net, *id, opts.prerelease).await,
    }
}

/// "v1.2.3" == "1.2.3" for update checks.
pub fn normalize_version(v: &str) -> String {
    v.trim().trim_start_matches(['v', 'V']).trim().to_ascii_lowercase()
}

/// Turn "https://github.com/owner/repo/..." or "owner/repo" into "owner/repo".
pub fn parse_github_repo(input: &str) -> Option<String> {
    let s = input.trim().trim_end_matches('/');
    let s = s
        .strip_prefix("https://github.com/")
        .or_else(|| s.strip_prefix("http://github.com/"))
        .or_else(|| s.strip_prefix("github.com/"))
        .or_else(|| s.strip_prefix("https://www.github.com/"))
        .unwrap_or(s);
    let parts: Vec<&str> = s.split('/').collect();
    if parts.len() < 2 {
        return None;
    }
    let ok = |p: &str| {
        !p.is_empty() && p != "." && p != ".." && p.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    };
    let (owner, repo) = (parts[0], parts[1].trim_end_matches(".git"));
    if ok(owner) && ok(repo) {
        Some(format!("{owner}/{repo}"))
    } else {
        None
    }
}

/// A source that came from outside the app (an imported list). Same rules as
/// typing it into the search box, and Wago is not a source any more.
pub fn checked_source(source: Source) -> Result<Source, String> {
    let bad = |what: &str, id: &str| Err(format!("{what} {id:?} in this list isn't valid"));
    match &source {
        Source::Github(repo) => {
            if parse_github_repo(repo).as_deref() != Some(repo.as_str()) {
                return bad("GitHub repo", repo);
            }
        }
        Source::Tukui(slug) => {
            if slug.is_empty() || !slug.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
                return bad("TukUI addon", slug);
            }
        }
        Source::Wago(_) => return Err("Wago isn't a source any more".into()),
        Source::Wowi(_) | Source::Curse(_) => {}
    }
    Ok(source)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn imported_sources_are_checked() {
        let github = |r: &str| Source::Github(r.into());
        assert!(checked_source(github("BigWigsMods/BigWigs")).is_ok());
        for repo in ["../../user", "a/b/../../c", "a/b?x=1", "https://github.com/a/b", "nope", "a/.."] {
            assert!(checked_source(github(repo)).is_err(), "{repo}");
        }
        assert!(checked_source(Source::Tukui("elvui".into())).is_ok());
        assert!(checked_source(Source::Tukui("../elvui".into())).is_err());
        assert!(checked_source(Source::Wago("abc".into())).is_err());
        assert!(checked_source(Source::Wowi(1)).is_ok());
        assert!(checked_source(Source::Curse(1)).is_ok());
    }

    #[test]
    fn published_hashes_are_taken_only_in_their_exact_shape() {
        let sha256 = "89F2BA623BB6D69194843EA26AB7EAFFAC368DC51F85DDF614C881C2B608F1B1";
        assert_eq!(FileHash::sha256(sha256).map(|h| h.hex().to_string()), Some(sha256.to_ascii_lowercase()));
        assert_eq!(
            FileHash::from_github_digest(&format!("sha256:{}", sha256.to_ascii_lowercase())),
            FileHash::sha256(sha256)
        );
        assert_eq!(FileHash::from_github_digest("sha512:abcd"), None, "only SHA-256 digests");
        assert_eq!(FileHash::from_github_digest(sha256), None, "the prefix is required");
        assert_eq!(FileHash::md5("e085da4ee8a90d82355f9ca1e7f9969a").map(|h| h.label()), Some("MD5"));
        assert_eq!(FileHash::sha1("da39a3ee5e6b4b0d3255bfef95601890afd80709").map(|h| h.label()), Some("SHA-1"));
        for bad in ["", "abc", "e085da4ee8a90d82355f9ca1e7f9969", "z085da4ee8a90d82355f9ca1e7f9969a", "e085da4ee8a90d82355f9ca1e7f9969a0"] {
            assert_eq!(FileHash::md5(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn parses_github_inputs() {
        assert_eq!(parse_github_repo("BigWigsMods/BigWigs").as_deref(), Some("BigWigsMods/BigWigs"));
        assert_eq!(parse_github_repo("https://github.com/BigWigsMods/BigWigs/releases/tag/v1").as_deref(), Some("BigWigsMods/BigWigs"));
        assert_eq!(parse_github_repo("github.com/a/b.git").as_deref(), Some("a/b"));
        assert_eq!(parse_github_repo("nope"), None);
        assert_eq!(parse_github_repo("https://gitlab.com/a/b"), None);
    }
}
