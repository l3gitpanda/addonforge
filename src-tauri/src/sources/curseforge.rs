//! CurseForge (api.curseforge.com), 3rd Party API.
//!
//! CurseForge needs an API key. There are two places one can come from:
//!
//! * the user's own key, entered in Settings and kept in the operating
//!   system's credential store (see `secrets.rs`). This is the normal case;
//! * a key baked into the build (see `build.rs`), for a maintainer whose
//!   agreement with CurseForge covers shipping one. The user's own key wins.
//!
//! With neither, CurseForge is switched off and every function here says so.
//! CurseForge issues each key to one developer and its terms forbid passing it
//! on, so a key is never written to the settings file, the log, diagnostics or
//! an exported list, and is sent only to CurseForge's own hosts.
//!
//! Rules we keep because their terms ask for them: API responses are not
//! cached on disk (the only cache is in memory for a few minutes; settings and
//! the log keep just the project id and version name of an install), there is
//! no relay server, authors who disallow third-party distribution are never
//! downloaded around (they stay link-only), and calls are kept to a minimum
//! (one batch request per update check, one request per search).

use super::Remote;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock, RwLock};
use std::time::{Duration, Instant};

include!(concat!(env!("OUT_DIR"), "/cf_key.rs"));

pub const BASE: &str = "https://api.curseforge.com/v1";
/// World of Warcraft's game id on CurseForge.
pub const GAME_ID: u64 = 1;
/// The "WoW Forever" game version type (slug `wow-forever`, version 1.60.1).
pub const FOREVER_TYPE: u64 = 88568;
pub const OPT_OUT: &str = "CURSE_OPTOUT";
/// What every CurseForge action answers when there is no key to use.
pub const NO_KEY: &str = "CurseForge needs an API key: add yours in Settings";
/// Where a CurseForge API key comes from.
pub const KEY_PAGE: &str = "https://console.curseforge.com/";
const API_HOST: &str = "api.curseforge.com";
/// CurseForge's file CDN. Download links point at `edge.forgecdn.net`, which
/// hands the request on to another host under the same domain.
const CDN_DOMAIN: &str = "forgecdn.net";
const KEY_HEADER: &str = "x-api-key";
const CACHE_TTL: Duration = Duration::from_secs(15 * 60);

/// Where the key in use came from.
#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum KeyOrigin {
    /// The user's own, from the OS credential store.
    User,
    /// Baked into this build.
    Embedded,
    None,
}

/// The user's own key for this run. Filled from the credential store at
/// startup and whenever the key is saved or removed in Settings.
static USER_KEY: RwLock<Option<String>> = RwLock::new(None);

/// Use `key` (the user's own) from now on; `None` goes back to the built-in
/// key, if this build has one.
pub fn set_user_key(key: Option<String>) {
    *USER_KEY.write().unwrap_or_else(|p| p.into_inner()) = key;
}

/// The user's own key wins over a built-in one.
fn pick_key(user: Option<&str>, embedded: &str) -> (Option<String>, KeyOrigin) {
    match user.filter(|k| !k.is_empty()) {
        Some(k) => (Some(k.to_string()), KeyOrigin::User),
        None if !embedded.is_empty() => (Some(embedded.to_string()), KeyOrigin::Embedded),
        None => (None, KeyOrigin::None),
    }
}

fn current_key() -> (Option<String>, KeyOrigin) {
    let user = USER_KEY.read().unwrap_or_else(|p| p.into_inner());
    pick_key(user.as_deref(), CF_KEY)
}

/// The key to use, if there is one.
pub fn key() -> Option<String> {
    current_key().0
}

pub fn key_origin() -> KeyOrigin {
    current_key().1
}

pub fn available() -> bool {
    key().is_some()
}

/// Tidy a key the user typed or pasted, or say why it cannot be one. Catches
/// paste accidents (a web address, a sentence, two lines) before anything is
/// sent or stored. The error never repeats what was typed.
pub fn checked_key(raw: &str) -> Result<String, String> {
    let key = raw.trim();
    if key.is_empty() {
        return Err("Paste your CurseForge API key first".into());
    }
    if !key.bytes().all(|b| b.is_ascii_graphic()) {
        return Err("That doesn't look like a CurseForge API key: it has spaces, line breaks or unusual characters in it".into());
    }
    if key.starts_with("http://") || key.starts_with("https://") {
        return Err("That is a web address, not an API key".into());
    }
    if !(20..=200).contains(&key.len()) {
        return Err("That doesn't look like a CurseForge API key: it is too short or too long".into());
    }
    Ok(key.to_string())
}

pub fn project_url(id: u64) -> String {
    format!("https://www.curseforge.com/projects/{id}")
}

#[derive(Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct ModInfo {
    pub id: u64,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub download_count: u64,
    #[serde(default)]
    pub allow_mod_distribution: Option<bool>,
    #[serde(default)]
    pub links: Links,
    #[serde(default)]
    pub authors: Vec<Author>,
    #[serde(default)]
    pub latest_files: Vec<FileInfo>,
    #[serde(default)]
    pub latest_files_indexes: Vec<FileIndex>,
    #[serde(default)]
    pub categories: Vec<Category>,
}

#[derive(Deserialize, Clone, Debug, Default)]
pub struct Category {
    #[serde(default)]
    pub name: String,
}

/// How a list page is ordered. Maps to CurseForge's `sortField`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Sort {
    Popularity,
    Downloads,
    DownloadsAscending,
    Updated,
    UpdatedAscending,
    Name,
    NameDescending,
}

impl Sort {
    pub fn parse(s: &str) -> Sort {
        match s {
            "downloads" => Sort::Downloads,
            "downloads-asc" => Sort::DownloadsAscending,
            "updated" => Sort::Updated,
            "updated-asc" => Sort::UpdatedAscending,
            "name" => Sort::Name,
            "name-desc" => Sort::NameDescending,
            _ => Sort::Popularity,
        }
    }
    fn query(self) -> &'static str {
        match self {
            Sort::Popularity => "sortField=2&sortOrder=desc",
            Sort::Downloads => "sortField=6&sortOrder=desc",
            Sort::DownloadsAscending => "sortField=6&sortOrder=asc",
            Sort::Updated => "sortField=3&sortOrder=desc",
            Sort::UpdatedAscending => "sortField=3&sortOrder=asc",
            Sort::Name => "sortField=4&sortOrder=asc",
            Sort::NameDescending => "sortField=4&sortOrder=desc",
        }
    }
}

/// One request's worth of the list: text filter (may be empty), order,
/// one CurseForge category id, and the offset into the result set.
#[derive(Clone, Debug)]
pub struct Page {
    pub query: String,
    pub sort: Sort,
    pub category: Option<u64>,
    pub index: u32,
    pub limit: usize,
}

#[derive(Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct Links {
    #[serde(default)]
    pub website_url: String,
}

#[derive(Deserialize, Clone, Debug, Default)]
pub struct Author {
    #[serde(default)]
    pub name: String,
}

#[derive(Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct FileInfo {
    pub id: u64,
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub file_name: String,
    /// 1 = release, 2 = beta, 3 = alpha.
    #[serde(default)]
    pub release_type: u8,
    #[serde(default)]
    pub file_date: String,
    #[serde(default)]
    pub download_url: Option<String>,
    #[serde(default)]
    pub sortable_game_versions: Vec<SortableVersion>,
}

#[derive(Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct SortableVersion {
    #[serde(default)]
    pub game_version_type_id: Option<u64>,
}

#[derive(Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct FileIndex {
    #[serde(default)]
    pub file_id: u64,
    #[serde(default)]
    pub release_type: u8,
    #[serde(default)]
    pub game_version_type_id: Option<u64>,
}

#[derive(Deserialize)]
struct Data<T> {
    data: T,
}

#[derive(Deserialize)]
struct Paged<T> {
    data: T,
    #[serde(default)]
    pagination: Pagination,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct Pagination {
    #[serde(default)]
    total_count: u64,
}

fn cache() -> &'static Mutex<HashMap<u64, (Instant, ModInfo)>> {
    static CACHE: OnceLock<Mutex<HashMap<u64, (Instant, ModInfo)>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn cached(id: u64) -> Option<ModInfo> {
    let map = cache().lock().ok()?;
    map.get(&id).filter(|(t, _)| t.elapsed() < CACHE_TTL).map(|(_, m)| m.clone())
}

fn remember(mods: &[ModInfo]) {
    if let Ok(mut map) = cache().lock() {
        let now = Instant::now();
        for m in mods {
            map.insert(m.id, (now, m.clone()));
        }
    }
}

/// Put mods in the in-memory cache as an update check would, for tests of
/// what other modules make of a listing.
#[cfg(test)]
pub fn remember_for_tests(mods: &[ModInfo]) {
    remember(mods)
}

fn request_with(client: &reqwest::Client, method: reqwest::Method, url: String, key: &str) -> reqwest::RequestBuilder {
    client
        .request(method, url)
        .header(KEY_HEADER, key)
        .header("Accept", "application/json")
        .timeout(Duration::from_secs(20))
}

fn request(client: &reqwest::Client, method: reqwest::Method, url: String) -> anyhow::Result<reqwest::RequestBuilder> {
    let key = key().ok_or_else(|| anyhow::anyhow!(NO_KEY))?;
    Ok(request_with(client, method, url, &key))
}

/// The two places a request may carry the key to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum KeyZone {
    Api,
    Cdn,
}

fn key_zone(host: &str) -> Option<KeyZone> {
    let host = host.to_ascii_lowercase();
    if host == API_HOST {
        Some(KeyZone::Api)
    } else if host == CDN_DOMAIN || host.strip_suffix(CDN_DOMAIN).is_some_and(|rest| rest.ends_with('.')) {
        Some(KeyZone::Cdn)
    } else {
        None
    }
}

/// reqwest strips Authorization on a cross-host redirect, but keeps custom headers
/// like `x-api-key`. So a request that may carry the key is never allowed to be
/// redirected out of where it started: the API stays on the API host, and a
/// download stays on CurseForge's CDN (which does redirect between its own hosts).
pub fn redirect_keeps_key_home(from_host: Option<&str>, to_host: Option<&str>) -> bool {
    match from_host.and_then(key_zone) {
        None => true,
        Some(zone) => to_host.and_then(key_zone) == Some(zone),
    }
}

/// Headers for downloading `url`. CurseForge's CDN asks for the API key on
/// file downloads (required since July 2026), so the key goes along when,
/// and only when, the link is an https link to that CDN.
pub fn download_headers(url: &str) -> Vec<(&'static str, String)> {
    match key() {
        Some(k) if is_cdn_download(url) => vec![(KEY_HEADER, k)],
        _ => Vec::new(),
    }
}

fn is_cdn_download(url: &str) -> bool {
    reqwest::Url::parse(url)
        .ok()
        .filter(|u| u.scheme() == "https")
        .and_then(|u| u.host_str().and_then(key_zone))
        == Some(KeyZone::Cdn)
}

/// Why a CurseForge download failed, in words that never include the download
/// link (nothing from the API but the installed version is kept on disk, and
/// this text goes to the log).
pub fn download_error(e: &anyhow::Error) -> String {
    let Some(http) = e.chain().find_map(|c| c.downcast_ref::<reqwest::Error>()) else {
        // Not a network error (disk, size limit): nothing of CurseForge's in it.
        return e.to_string();
    };
    match http.status().map(|s| s.as_u16()) {
        Some(401 | 403) => "CurseForge refused the download: it did not accept the API key. Check the key in Settings".into(),
        Some(404) => "CurseForge no longer has that file; check for updates again".into(),
        Some(429) => "CurseForge rate limit hit; try again in a minute".into(),
        Some(code) => format!("CurseForge download failed (HTTP {code})"),
        None if http.is_timeout() => "CurseForge download timed out".into(),
        None if http.is_redirect() => "CurseForge download failed: it redirected away from CurseForge's own hosts".into(),
        None if http.is_connect() => "Couldn't connect to CurseForge's download server".into(),
        None => "CurseForge download failed".into(),
    }
}

fn api_error(status: u16) -> Option<anyhow::Error> {
    match status {
        401 | 403 => Some(anyhow::anyhow!("CurseForge rejected the API key; check it in Settings")),
        404 => Some(anyhow::anyhow!("CurseForge has no such addon")),
        429 => Some(anyhow::anyhow!("CurseForge rate limit hit; try again in a minute")),
        _ => None,
    }
}

/// What CurseForge said about a key it was shown.
fn key_verdict(status: u16) -> Result<(), String> {
    match status {
        200..=299 => Ok(()),
        401 | 403 => Err("CurseForge did not accept that key for World of Warcraft. Check that it was copied whole, and that it is a key for the CurseForge API".into()),
        429 => Err("CurseForge rate limit hit; try again in a minute".into()),
        code => Err(format!("CurseForge answered HTTP {code} when the key was tested; nothing was saved")),
    }
}

/// One request that shows whether CurseForge accepts `key` for World of
/// Warcraft. It uses the key it is given, not the one in use, so a key is only
/// stored once it has worked.
pub async fn verify_key(client: &reqwest::Client, key: &str) -> Result<(), String> {
    let resp = request_with(client, reqwest::Method::GET, format!("{BASE}/games/{GAME_ID}"), key)
        .send()
        .await
        // The request's address is fixed and has no key in it, but keep the
        // message to what the user can act on.
        .map_err(|e| {
            if e.is_timeout() || e.is_connect() {
                "Couldn't reach CurseForge to test the key; nothing was saved".to_string()
            } else {
                format!("Couldn't test the key with CurseForge ({}); nothing was saved", e.without_url())
            }
        })?;
    key_verdict(resp.status().as_u16())
}

pub async fn fetch(client: &reqwest::Client, id: u64) -> anyhow::Result<ModInfo> {
    if let Some(m) = cached(id) {
        return Ok(m);
    }
    let resp = request(client, reqwest::Method::GET, format!("{BASE}/mods/{id}"))?.send().await?;
    if let Some(e) = api_error(resp.status().as_u16()) {
        return Err(e);
    }
    let m: Data<ModInfo> = crate::net::json(resp.error_for_status()?).await?;
    remember(std::slice::from_ref(&m.data));
    Ok(m.data)
}

/// One request for many ids; what comes back is kept in memory for a while.
pub async fn prefetch(client: &reqwest::Client, ids: &[u64]) -> anyhow::Result<usize> {
    let want: Vec<u64> = ids.iter().copied().filter(|id| cached(*id).is_none()).collect();
    if want.is_empty() {
        return Ok(0);
    }
    let mut total = 0;
    for chunk in want.chunks(50) {
        let resp = request(client, reqwest::Method::POST, format!("{BASE}/mods"))?
            .json(&serde_json::json!({ "modIds": chunk, "filterPcOnly": true }))
            .send()
            .await?;
        if let Some(e) = api_error(resp.status().as_u16()) {
            return Err(e);
        }
        let m: Data<Vec<ModInfo>> = crate::net::json(resp.error_for_status()?).await?;
        remember(&m.data);
        total += m.data.len();
    }
    Ok(total)
}

/// Forever addons matching `query`, most popular first. Not cached.
pub async fn search(client: &reqwest::Client, query: &str, limit: usize) -> anyhow::Result<Vec<ModInfo>> {
    let page = Page { query: query.into(), sort: Sort::Popularity, category: None, index: 0, limit };
    Ok(search_page(client, &page).await?.0)
}

/// One page of Forever addons plus CurseForge's total count for the
/// filter. An empty query is the whole list. One request per call,
/// nothing kept beyond the 15-minute in-memory mod cache.
pub async fn search_page(client: &reqwest::Client, page: &Page) -> anyhow::Result<(Vec<ModInfo>, u64)> {
    let q = page.query.trim();
    if !q.is_empty() && q.len() < 2 {
        return Ok((Vec::new(), 0));
    }
    let mut url = format!(
        "{BASE}/mods/search?gameId={GAME_ID}&gameVersionTypeId={FOREVER_TYPE}&{}&pageSize={}&index={}",
        page.sort.query(),
        page.limit.clamp(1, 50),
        page.index.min(9950)
    );
    if let Some(c) = page.category {
        url.push_str(&format!("&categoryId={c}"));
    }
    if !q.is_empty() {
        url.push_str("&searchFilter=");
        url.push_str(&urlencoding(q));
    }
    let resp = request(client, reqwest::Method::GET, url)?.send().await?;
    if let Some(e) = api_error(resp.status().as_u16()) {
        return Err(e);
    }
    let m: Paged<Vec<ModInfo>> = crate::net::json(resp.error_for_status()?).await?;
    remember(&m.data);
    Ok((m.data, m.pagination.total_count))
}

fn urlencoding(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// What `pick_file` decided about a mod.
#[derive(Debug, PartialEq)]
pub enum Pick {
    /// A Forever file with a download link was in the mod's `latestFiles`.
    Ready(Remote),
    /// The Forever file exists (by index) but its details need one more request.
    NeedsFile(u64),
    /// The author has switched off third-party downloads.
    OptOut,
    /// Nothing published for Forever.
    NoForeverFile,
}

fn release_order(prerelease: bool) -> [u8; 3] {
    if prerelease { [2, 1, 3] } else { [1, 2, 3] }
}

fn is_forever(f: &FileInfo) -> bool {
    f.sortable_game_versions.iter().any(|v| v.game_version_type_id == Some(FOREVER_TYPE))
}

fn remote_from(f: &FileInfo) -> Option<Remote> {
    let url = f.download_url.clone().filter(|u| u.starts_with("https://"))?;
    Some(Remote {
        version: f.display_name.clone(),
        download_url: url,
        filename: if f.file_name.is_empty() { format!("curse-{}.zip", f.id) } else { f.file_name.clone() },
        forever: Some(true),
        prerelease: f.release_type != 1,
    })
}

/// Choose the Forever file for a mod. Stable first unless the user allows
/// pre-releases; the newest file of the chosen kind wins.
pub fn pick_file(m: &ModInfo, prerelease: bool) -> Pick {
    if m.allow_mod_distribution == Some(false) {
        return Pick::OptOut;
    }
    for kind in release_order(prerelease) {
        let best = m
            .latest_files
            .iter()
            .filter(|f| f.release_type == kind && is_forever(f))
            .max_by(|a, b| a.file_date.cmp(&b.file_date).then(a.id.cmp(&b.id)));
        if let Some(f) = best {
            return match remote_from(f) {
                Some(r) => Pick::Ready(r),
                None => Pick::OptOut,
            };
        }
        if let Some(ix) = m
            .latest_files_indexes
            .iter()
            .filter(|i| i.release_type == kind && i.game_version_type_id == Some(FOREVER_TYPE))
            .max_by_key(|i| i.file_id)
        {
            return Pick::NeedsFile(ix.file_id);
        }
    }
    Pick::NoForeverFile
}

/// What CurseForge lists for an addon, for rows we are not allowed to
/// download: the published name of the newest Forever file and nothing else.
/// Never a download link.
#[derive(Clone, Debug, PartialEq)]
pub struct Listing {
    pub version: String,
    pub prerelease: bool,
}

/// The newest Forever file CurseForge lists for `m`, whether or not the author
/// allows third-party downloads. Reads the mod we already hold: no request.
pub fn latest_listing(m: &ModInfo, prerelease: bool) -> Option<Listing> {
    for kind in release_order(prerelease) {
        let best = m
            .latest_files
            .iter()
            .filter(|f| f.release_type == kind && is_forever(f))
            .max_by(|a, b| a.file_date.cmp(&b.file_date).then(a.id.cmp(&b.id)));
        if let Some(f) = best {
            let name = match f.display_name.trim() {
                "" => f.file_name.trim().trim_end_matches(".zip"),
                n => n,
            };
            if !name.is_empty() {
                return Some(Listing { version: name.to_string(), prerelease: f.release_type != 1 });
            }
        }
    }
    None
}

/// The mod as last fetched, while it is still in the in-memory cache. Used by
/// link-only rows, which read the update-check batch instead of asking again.
pub fn cached_info(id: u64) -> Option<ModInfo> {
    cached(id)
}

pub async fn file(client: &reqwest::Client, mod_id: u64, file_id: u64) -> anyhow::Result<FileInfo> {
    let resp = request(client, reqwest::Method::GET, format!("{BASE}/mods/{mod_id}/files/{file_id}"))?
        .send()
        .await?;
    if let Some(e) = api_error(resp.status().as_u16()) {
        return Err(e);
    }
    let f: Data<FileInfo> = crate::net::json(resp.error_for_status()?).await?;
    Ok(f.data)
}

pub async fn resolve(client: &reqwest::Client, id: u64, prerelease: bool) -> anyhow::Result<Remote> {
    let m = fetch(client, id).await?;
    match pick_file(&m, prerelease) {
        Pick::Ready(r) => Ok(r),
        Pick::NeedsFile(fid) => {
            let f = file(client, id, fid).await?;
            remote_from(&f).ok_or_else(|| anyhow::anyhow!("{OPT_OUT}: the author only distributes this addon through CurseForge's own app"))
        }
        Pick::OptOut => anyhow::bail!("{OPT_OUT}: the author only distributes this addon through CurseForge's own app"),
        Pick::NoForeverFile => anyhow::bail!("CurseForge has no Forever release for this addon"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_key_never_follows_a_redirect_off_the_api_host() {
        let api = Some("api.curseforge.com");
        assert!(redirect_keeps_key_home(api, api));
        assert!(!redirect_keeps_key_home(api, Some("evil.example")));
        assert!(!redirect_keeps_key_home(api, None));
        assert!(!redirect_keeps_key_home(api, Some("edge.forgecdn.net")), "an API call has no business on the CDN");
        assert!(redirect_keeps_key_home(Some("github.com"), Some("objects.githubusercontent.com")));
    }

    #[test]
    fn a_download_may_move_between_cdn_hosts_but_never_leave_them() {
        let edge = Some("edge.forgecdn.net");
        // What the CDN really does: edge hands the request to mediafilez.
        assert!(redirect_keeps_key_home(edge, Some("mediafilez.forgecdn.net")));
        assert!(redirect_keeps_key_home(Some("mediafilez.forgecdn.net"), edge));
        for elsewhere in ["evil.example", "forgecdn.net.evil.example", "evilforgecdn.net", "api.curseforge.com", "s3.amazonaws.com"] {
            assert!(!redirect_keeps_key_home(edge, Some(elsewhere)), "{elsewhere}");
        }
        assert!(!redirect_keeps_key_home(edge, None));
    }

    #[test]
    fn the_key_goes_with_a_download_only_to_the_cdn_over_https() {
        for yes in [
            "https://edge.forgecdn.net/files/1/2/Addon.zip",
            "https://mediafilez.forgecdn.net/files/1/2/Addon.zip",
            "https://EDGE.ForgeCDN.net/files/1/2/Addon.zip",
        ] {
            assert!(is_cdn_download(yes), "{yes}");
        }
        for no in [
            "http://edge.forgecdn.net/files/1/2/Addon.zip",
            "https://github.com/a/b/releases/download/v1/b.zip",
            "https://forgecdn.net.evil.example/files/1/2/Addon.zip",
            "https://evilforgecdn.net/files/1/2/Addon.zip",
            "https://evil.example/?u=https://edge.forgecdn.net/x.zip",
            "https://edge.forgecdn.net@evil.example/x.zip",
            "https://api.curseforge.com/v1/mods/1",
            "not a url",
        ] {
            assert!(!is_cdn_download(no), "{no}");
        }
    }

    #[test]
    fn the_users_own_key_wins_over_a_built_in_one() {
        assert_eq!(pick_key(Some("mine"), "built-in"), (Some("mine".into()), KeyOrigin::User));
        assert_eq!(pick_key(Some("mine"), ""), (Some("mine".into()), KeyOrigin::User));
        assert_eq!(pick_key(None, "built-in"), (Some("built-in".into()), KeyOrigin::Embedded));
        assert_eq!(pick_key(Some(""), "built-in"), (Some("built-in".into()), KeyOrigin::Embedded));
        assert_eq!(pick_key(None, ""), (None, KeyOrigin::None));
        assert_eq!(serde_json::to_string(&KeyOrigin::User).unwrap(), "\"user\"");
        assert_eq!(serde_json::to_string(&KeyOrigin::None).unwrap(), "\"none\"");
    }

    #[test]
    fn a_pasted_key_is_tidied_or_refused_without_being_repeated() {
        let good = "$2a$10$abcdefghijklmnopqrstuvABCDEFGHIJKLMNOPQRSTUVWXYZ012345";
        assert_eq!(checked_key(&format!("  {good}\r\n")).as_deref(), Ok(good));
        for bad in [
            "",
            "   ",
            "abc123",
            "two words that are long enough to pass a length check",
            "line-one-of-a-key-that-is-long\nline-two-of-a-key-that-is-long",
            "https://console.curseforge.com/?#/api-keys",
            "kéy-with-a-letter-that-is-not-ascii-in-it",
            &"x".repeat(201),
        ] {
            let why = checked_key(bad).expect_err(bad);
            let typed = bad.trim();
            assert!(typed.is_empty() || !why.contains(typed), "the message repeats what was typed: {why}");
        }
    }

    #[test]
    fn a_key_is_only_good_when_curseforge_says_so() {
        assert!(key_verdict(200).is_ok());
        for refused in [401, 403] {
            assert!(key_verdict(refused).unwrap_err().contains("did not accept"));
        }
        assert!(key_verdict(429).unwrap_err().contains("rate limit"));
        assert!(key_verdict(500).unwrap_err().contains("nothing was saved"));
        assert!(key_verdict(302).is_err(), "a redirect is not a yes");
    }

    #[test]
    fn a_failed_download_is_explained_without_its_link() {
        // Not a network error: passed through as it is.
        let disk = anyhow::anyhow!("download exceeded 512 MB; refusing");
        assert_eq!(download_error(&disk), "download exceeded 512 MB; refusing");
        // A network error: the link in the context never reaches the message.
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let http = rt.block_on(async {
            // Nothing listens on port 1 of this machine: the request fails without leaving it.
            reqwest::Client::new().get("http://127.0.0.1:1/files/1/2/Secret-Addon.zip").send().await.unwrap_err()
        });
        let e = anyhow::Error::new(http).context("download failed: http://127.0.0.1:1/files/1/2/Secret-Addon.zip");
        let msg = download_error(&e);
        assert!(msg.contains("CurseForge"), "{msg}");
        assert!(!msg.contains("Secret-Addon") && !msg.contains("127.0.0.1"), "{msg}");
    }

    fn file(id: u64, name: &str, kind: u8, types: &[u64], url: Option<&str>, date: &str) -> FileInfo {
        FileInfo {
            id,
            display_name: name.into(),
            file_name: format!("{name}.zip"),
            release_type: kind,
            file_date: date.into(),
            download_url: url.map(String::from),
            sortable_game_versions: types.iter().map(|t| SortableVersion { game_version_type_id: Some(*t) }).collect(),
        }
    }

    #[test]
    fn search_sort_directions_reach_the_api() {
        for (value, expected) in [
            ("downloads", "sortField=6&sortOrder=desc"),
            ("downloads-asc", "sortField=6&sortOrder=asc"),
            ("updated", "sortField=3&sortOrder=desc"),
            ("updated-asc", "sortField=3&sortOrder=asc"),
            ("name", "sortField=4&sortOrder=asc"),
            ("name-desc", "sortField=4&sortOrder=desc"),
        ] {
            assert_eq!(Sort::parse(value).query(), expected);
        }
    }

    #[test]
    fn picks_the_newest_stable_forever_file_with_a_link() {
        let m = ModInfo {
            allow_mod_distribution: Some(true),
            latest_files: vec![
                file(1, "v11", 1, &[517], Some("https://edge.forgecdn.net/a.zip"), "2026-01-01"),
                file(2, "v12.3.0", 1, &[517, FOREVER_TYPE], Some("https://edge.forgecdn.net/b.zip"), "2026-10-01"),
                file(3, "v12.3.1", 1, &[FOREVER_TYPE], Some("https://edge.forgecdn.net/c.zip"), "2026-10-20"),
                file(4, "v12.4-beta", 2, &[FOREVER_TYPE], Some("https://edge.forgecdn.net/d.zip"), "2026-10-25"),
            ],
            ..Default::default()
        };
        match pick_file(&m, false) {
            Pick::Ready(r) => {
                assert_eq!(r.version, "v12.3.1");
                assert_eq!(r.forever, Some(true));
                assert!(!r.prerelease);
            }
            other => panic!("{other:?}"),
        }
        match pick_file(&m, true) {
            Pick::Ready(r) => assert_eq!(r.version, "v12.4-beta"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn opted_out_authors_are_never_downloaded() {
        let m = ModInfo {
            allow_mod_distribution: Some(false),
            latest_files: vec![file(9, "x", 1, &[FOREVER_TYPE], Some("https://edge.forgecdn.net/x.zip"), "2026-10-01")],
            ..Default::default()
        };
        assert_eq!(pick_file(&m, false), Pick::OptOut);
        // Allowed on paper but no link on the file: treat the same way.
        let m2 = ModInfo {
            allow_mod_distribution: Some(true),
            latest_files: vec![file(9, "x", 1, &[FOREVER_TYPE], None, "2026-10-01")],
            ..Default::default()
        };
        assert_eq!(pick_file(&m2, false), Pick::OptOut);
    }

    #[test]
    fn falls_back_to_the_index_when_latest_files_lacks_forever() {
        let m = ModInfo {
            allow_mod_distribution: Some(true),
            latest_files: vec![file(1, "retail", 1, &[517], Some("https://edge.forgecdn.net/a.zip"), "2026-10-01")],
            latest_files_indexes: vec![
                FileIndex { file_id: 70, release_type: 3, game_version_type_id: Some(FOREVER_TYPE) },
                FileIndex { file_id: 71, release_type: 1, game_version_type_id: Some(FOREVER_TYPE) },
                FileIndex { file_id: 72, release_type: 1, game_version_type_id: Some(517) },
            ],
            ..Default::default()
        };
        assert_eq!(pick_file(&m, false), Pick::NeedsFile(71));
        let none = ModInfo { allow_mod_distribution: Some(true), ..Default::default() };
        assert_eq!(pick_file(&none, false), Pick::NoForeverFile);
    }

    #[test]
    fn an_opted_out_addon_still_says_what_was_published() {
        // No download link anywhere, but the version name is public information.
        let m = ModInfo {
            allow_mod_distribution: Some(false),
            latest_files: vec![
                file(1, "Bartender4-4.15.9", 1, &[FOREVER_TYPE], None, "2026-10-01"),
                file(2, "Bartender4-4.16.0", 1, &[FOREVER_TYPE], None, "2026-11-02"),
                file(3, "Bartender4-4.16.1-beta", 2, &[FOREVER_TYPE], None, "2026-11-04"),
                file(4, "Bartender4-retail", 1, &[517], None, "2026-11-05"),
            ],
            ..Default::default()
        };
        assert_eq!(pick_file(&m, false), Pick::OptOut, "never downloadable");
        let stable = latest_listing(&m, false).unwrap();
        assert_eq!(stable.version, "Bartender4-4.16.0");
        assert!(!stable.prerelease);
        assert_eq!(latest_listing(&m, true).unwrap().version, "Bartender4-4.16.1-beta");
        // Nothing published for Forever: nothing to say.
        let retail_only = ModInfo {
            latest_files: vec![file(5, "x", 1, &[517], None, "2026-11-05")],
            ..Default::default()
        };
        assert_eq!(latest_listing(&retail_only, false), None);
    }

    #[test]
    fn key_is_absent_in_test_builds_or_round_trips() {
        // Either no key (CI, contributors) or a masked key that unmasks to text.
        match key() {
            None => assert!(!available()),
            Some(k) => assert!(available() && !k.is_empty() && !k.contains('\n')),
        }
        assert_eq!(urlencoding("Leatrix Plus & Maps"), "Leatrix+Plus+%26+Maps");
    }
}
