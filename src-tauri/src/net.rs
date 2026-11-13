//! Every request the app makes goes through [`Net`], the one place that
//! decides whether it may be sent.
//!
//! Each request names its [`Purpose`]. Before anything is sent, and again on
//! every redirect hop, the gate checks the user's network settings
//! ([`Policy`]: offline mode and the per-source and per-feature switches) and
//! the hosts that purpose may reach. A refused request never opens a
//! connection, and a redirect to a host the purpose may not use is stopped
//! before it is followed. Everything sent or refused goes into a short list in
//! memory for the Network section of Settings; none of it is written to disk.
//!
//! Reading reply bodies with a size limit lives here too, so a server (or
//! anything between it and us) cannot make the app hold an arbitrarily large
//! reply in memory. Addon zips stream to disk instead and have their own cap in
//! `install::download`.

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use std::time::Duration;

// ---------------------------------------------------------------- purposes and hosts

/// What a request is for. The gate decides by it, and the activity list is
/// grouped by it.
#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Purpose {
    /// The signed addon catalogue and its signature, from the fork's repository.
    Catalogue,
    /// Download totals for the catalogue, from the fork's repository.
    Stats,
    /// The app's own update manifest and exe. Dormant while the fork has no
    /// update channel: no request is made.
    AppUpdate,
    CurseForge,
    GitHub,
    WoWInterface,
    Tukui,
    /// Dormant since Wago's updater keys went Patreon-only: no request is made.
    Wago,
    /// README text and release notes for an addon's details (GitHub).
    Details,
    /// Avatars, catalogue icons and README preview images.
    Images,
}

const PURPOSES: usize = 10;

impl Purpose {
    pub const ALL: [Purpose; PURPOSES] = [
        Purpose::Catalogue,
        Purpose::Stats,
        Purpose::AppUpdate,
        Purpose::CurseForge,
        Purpose::GitHub,
        Purpose::WoWInterface,
        Purpose::Tukui,
        Purpose::Wago,
        Purpose::Details,
        Purpose::Images,
    ];

    /// The start of a sentence about one of these requests.
    fn subject(self) -> &'static str {
        match self {
            Purpose::Catalogue => "The catalogue refresh",
            Purpose::Stats => "The download totals",
            Purpose::AppUpdate => "The app update check",
            Purpose::CurseForge => "A CurseForge request",
            Purpose::GitHub => "A GitHub request",
            Purpose::WoWInterface => "A WoWInterface request",
            Purpose::Tukui => "A Tukui request",
            Purpose::Wago => "A Wago request",
            Purpose::Details => "The addon details",
            Purpose::Images => "An image",
        }
    }

    /// The hosts these requests may reach, on the first request and on every
    /// redirect. Checked against what each site actually serves from (13
    /// November 2026): GitHub release files redirect from `github.com` to
    /// `release-assets.githubusercontent.com`, tagged source zips to
    /// `codeload.github.com`, avatars to `avatars.githubusercontent.com`;
    /// CurseForge downloads go from `edge.forgecdn.net` to
    /// `mediafilez.forgecdn.net`; WoWInterface serves zips from
    /// `cdn.wowinterface.com` and Tukui from `api.tukui.org`, neither with a
    /// redirect.
    fn rules(self) -> &'static [Rule] {
        use Rule::{Domain, Exact};
        match self {
            Purpose::Catalogue | Purpose::Stats => &[Exact("raw.githubusercontent.com")],
            Purpose::AppUpdate | Purpose::GitHub => &[Domain("github.com"), Domain("githubusercontent.com")],
            Purpose::Details => &[Exact("api.github.com"), Exact("raw.githubusercontent.com")],
            Purpose::CurseForge => &[Exact("api.curseforge.com"), Domain("forgecdn.net")],
            Purpose::WoWInterface => &[Domain("mmoui.com"), Domain("wowinterface.com")],
            Purpose::Tukui => &[Domain("tukui.org")],
            Purpose::Wago => &[Domain("wago.io")],
            Purpose::Images => &[
                Exact("github.com"),
                Domain("githubusercontent.com"),
                Exact("media.forgecdn.net"),
                Domain("wago.io"),
                Exact("www.wowinterface.com"),
                Exact("cdn-wow.mmoui.com"),
            ],
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum Rule {
    /// This host and every host under it: `github.com` covers `api.github.com`.
    Domain(&'static str),
    /// This host only.
    Exact(&'static str),
}

impl Rule {
    fn matches(self, host: &str) -> bool {
        match self {
            Rule::Exact(h) => host == h,
            Rule::Domain(d) => host == d || host.strip_suffix(d).is_some_and(|rest| rest.ends_with('.')),
        }
    }
}

/// Could a request for `purpose` go to `url` at all, whatever the switches say?
/// For a link found in data (a README image, a catalogue icon), before it is
/// handed to the gate.
pub fn allowed_host(purpose: Purpose, url: &str) -> bool {
    reqwest::Url::parse(url).is_ok_and(|u| check_address(&Hosts::default(), purpose, &u).is_ok())
}

// ---------------------------------------------------------------- the user's settings

/// The user's network settings, kept in `state.json`. Every switch starts on
/// and offline mode off, so a settings file from before they existed behaves
/// as it did.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(default)]
pub struct Policy {
    /// No request of any kind.
    pub offline: bool,
    /// Read the catalogue from the fork's repository; off, the built-in copy is used.
    pub catalogue: bool,
    /// Read download totals for the catalogue.
    pub stats: bool,
    /// Check installed addons for updates when the app opens. Not a request
    /// type: the update check obeys the source switches either way.
    pub launch_check: bool,
    /// Avatars, catalogue icons, README preview images.
    pub images: bool,
    pub curseforge: bool,
    pub github: bool,
    pub wowinterface: bool,
    pub tukui: bool,
}

impl Default for Policy {
    fn default() -> Self {
        Policy {
            offline: false,
            catalogue: true,
            stats: true,
            launch_check: true,
            images: true,
            curseforge: true,
            github: true,
            wowinterface: true,
            tukui: true,
        }
    }
}

impl Policy {
    /// The switches by their command-line names (`--cli network <name> on|off`).
    pub const SWITCHES: [&'static str; 9] =
        ["offline", "catalogue", "stats", "launch-check", "images", "curseforge", "github", "wowinterface", "tukui"];

    fn switch(&mut self, name: &str) -> Option<&mut bool> {
        Some(match name {
            "offline" => &mut self.offline,
            "catalogue" => &mut self.catalogue,
            "stats" => &mut self.stats,
            "launch-check" => &mut self.launch_check,
            "images" => &mut self.images,
            "curseforge" => &mut self.curseforge,
            "github" => &mut self.github,
            "wowinterface" => &mut self.wowinterface,
            "tukui" => &mut self.tukui,
            _ => return None,
        })
    }

    /// Set one switch by its command-line name. False when there is no such switch.
    pub fn set(&mut self, name: &str, on: bool) -> bool {
        self.switch(name).map(|s| *s = on).is_some()
    }

    /// One line for the log and diagnostics: "online, everything on",
    /// "online; off: GitHub, images" or "offline mode".
    pub fn summary(&self) -> String {
        if self.offline {
            return "offline mode".into();
        }
        let mut copy = self.clone();
        let off: Vec<&str> = Self::SWITCHES[1..]
            .iter()
            .copied()
            .filter(|name| copy.switch(name).is_some_and(|on| !*on))
            .collect();
        if off.is_empty() {
            "online, everything on".into()
        } else {
            format!("online; off: {}", off.join(", "))
        }
    }

    /// The switch, by its name in Settings, that keeps `purpose` from going out.
    fn switched_off(&self, purpose: Purpose) -> Option<&'static str> {
        let (on, name) = match purpose {
            Purpose::Catalogue => (self.catalogue, "Catalogue refresh"),
            Purpose::Stats => (self.stats, "Download totals"),
            // No switch: dormant, and governed by offline mode alone.
            Purpose::AppUpdate | Purpose::Wago => return None,
            Purpose::CurseForge => (self.curseforge, "CurseForge"),
            Purpose::GitHub | Purpose::Details => (self.github, "GitHub"),
            Purpose::WoWInterface => (self.wowinterface, "WoWInterface"),
            Purpose::Tukui => (self.tukui, "Tukui"),
            Purpose::Images => (self.images, "Remote images"),
        };
        (!on).then_some(name)
    }

    /// Images come from the addon sites. A site that is switched off stays
    /// off for its images too.
    fn image_site_off(&self, host: &str) -> Option<&'static str> {
        let under = |d: &'static str| Rule::Domain(d).matches(host);
        if (under("github.com") || under("githubusercontent.com")) && !self.github {
            Some("GitHub")
        } else if under("forgecdn.net") && !self.curseforge {
            Some("CurseForge")
        } else if (under("wowinterface.com") || under("mmoui.com")) && !self.wowinterface {
            Some("WoWInterface")
        } else {
            None
        }
    }
}

// ---------------------------------------------------------------- refusals

/// Why the gate refused a request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reason {
    Offline,
    /// The switch, by its name in Settings.
    SwitchedOff(&'static str),
    /// Not one of the hosts this purpose may use, or an address with a user name in it.
    Host,
    /// Not https.
    Scheme,
}

impl Reason {
    /// A few words for the activity list.
    fn short(&self) -> String {
        match self {
            Reason::Offline => "offline mode".into(),
            Reason::SwitchedOff(name) => format!("{name} is off"),
            Reason::Host => "host not allowed".into(),
            Reason::Scheme => "not https".into(),
        }
    }

    /// The note on an installed addon whose update check was skipped.
    pub fn not_checked(&self) -> String {
        match self {
            Reason::Offline => "Not checked: offline mode is on (Settings, Network).".into(),
            Reason::SwitchedOff(name) => format!("Not checked: {name} is switched off in Settings, Network."),
            Reason::Host | Reason::Scheme => "Not checked.".into(),
        }
    }
}

/// A request the gate refused. Nothing was sent.
#[derive(Debug, Clone)]
pub struct Refused {
    pub purpose: Purpose,
    pub host: String,
    pub reason: Reason,
}

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let host = if self.host.is_empty() { "that address" } else { self.host.as_str() };
        match &self.reason {
            Reason::Offline => write!(f, "Offline mode is on, so AddonForge did not contact {host}. Turn it off in Settings, Network."),
            Reason::SwitchedOff(name) => {
                write!(f, "{name} is switched off in Settings, Network, so AddonForge did not contact {host}.")
            }
            Reason::Host => write!(
                f,
                "{} tried to reach {host}, which is not one of the hosts it may use, so AddonForge stopped it.",
                self.purpose.subject()
            ),
            Reason::Scheme => write!(
                f,
                "{} tried to use an address that is not https ({host}), so AddonForge stopped it.",
                self.purpose.subject()
            ),
        }
    }
}

impl std::error::Error for Refused {}

/// Which hosts the gate allows: the table in [`Purpose::rules`], or for tests
/// a fixed list of local hosts with plain http.
#[derive(Clone, Debug, Default)]
struct Hosts {
    only: Option<Vec<String>>,
    plain_http: bool,
}

/// The whole decision for one address, on the first request and every redirect hop.
fn decide(policy: &Policy, hosts: &Hosts, purpose: Purpose, url: &reqwest::Url) -> Result<(), Reason> {
    if policy.offline {
        return Err(Reason::Offline);
    }
    if let Some(name) = policy.switched_off(purpose) {
        return Err(Reason::SwitchedOff(name));
    }
    check_address(hosts, purpose, url)?;
    if purpose == Purpose::Images && hosts.only.is_none() {
        let host = url.host_str().unwrap_or_default().to_ascii_lowercase();
        if let Some(name) = policy.image_site_off(&host) {
            return Err(Reason::SwitchedOff(name));
        }
    }
    Ok(())
}

/// The address part of the decision: scheme, no user name, an allowed host.
fn check_address(hosts: &Hosts, purpose: Purpose, url: &reqwest::Url) -> Result<(), Reason> {
    let plain_ok = hosts.plain_http && url.scheme() == "http";
    if url.scheme() != "https" && !plain_ok {
        return Err(Reason::Scheme);
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(Reason::Host);
    }
    let host = url.host_str().unwrap_or_default().to_ascii_lowercase();
    let allowed = match &hosts.only {
        Some(list) => list.iter().any(|h| *h == host),
        None => purpose.rules().iter().any(|r| r.matches(&host)),
    };
    if allowed {
        Ok(())
    } else {
        Err(Reason::Host)
    }
}

// ---------------------------------------------------------------- activity

/// Entries kept; older ones drop off.
const ACTIVITY_LEN: usize = 300;

/// One request, one redirect hop, or one refusal.
#[derive(Serialize, Clone, Debug)]
pub struct Entry {
    /// Milliseconds since 1970, for the panel to show in local time.
    pub at: u64,
    pub purpose: Purpose,
    pub host: String,
    /// The path only: no query string.
    pub path: String,
    /// The HTTP status ("200", "302"), "refused" or "failed".
    pub outcome: String,
    /// Where a redirect went, why a request was refused, or how it failed.
    pub detail: String,
}

#[derive(Serialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Count {
    /// Requests sent, each redirect hop counted.
    pub sent: u32,
    /// Requests the gate refused.
    pub refused: u32,
}

#[derive(Default)]
struct Activity {
    entries: VecDeque<Entry>,
    counts: [Count; PURPOSES],
}

#[derive(Serialize, Clone, Debug)]
pub struct PurposeCount {
    pub purpose: Purpose,
    #[serde(flatten)]
    pub count: Count,
}

/// What the Network section shows.
#[derive(Serialize, Clone, Debug)]
pub struct Snapshot {
    pub policy: Policy,
    /// Newest first.
    pub entries: Vec<Entry>,
    /// Since the app started, for every purpose that sent or refused anything.
    pub counts: Vec<PurposeCount>,
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// The path, cut to a length a list row can show.
fn short_path(url: &reqwest::Url) -> String {
    const MAX: usize = 120;
    let path = url.path();
    if path.len() <= MAX {
        return path.to_string();
    }
    let mut end = MAX;
    while !path.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}\u{2026}", &path[..end])
}

// ---------------------------------------------------------------- the gate

struct Shared {
    policy: RwLock<Policy>,
    hosts: Hosts,
    activity: Mutex<Activity>,
}

impl Shared {
    fn policy(&self) -> Policy {
        self.policy.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    fn decide(&self, purpose: Purpose, url: &reqwest::Url) -> Result<(), Reason> {
        decide(&self.policy(), &self.hosts, purpose, url)
    }

    fn record(&self, purpose: Purpose, url: &reqwest::Url, outcome: &str, detail: impl Into<String>, refused: bool) {
        let entry = Entry {
            at: now_ms(),
            purpose,
            host: url.host_str().unwrap_or_default().to_string(),
            path: short_path(url),
            outcome: outcome.to_string(),
            detail: detail.into(),
        };
        let mut activity = self.activity.lock().unwrap_or_else(|e| e.into_inner());
        let count = &mut activity.counts[purpose as usize];
        if refused {
            count.refused += 1;
        } else {
            count.sent += 1;
        }
        if activity.entries.len() >= ACTIVITY_LEN {
            activity.entries.pop_front();
        }
        activity.entries.push_back(entry);
    }
}

/// The HTTP client behind the gate. One `reqwest` client per purpose, built
/// on first use, so each one's redirect check knows what its requests are for.
pub struct Net {
    shared: Arc<Shared>,
    clients: [OnceLock<reqwest::Client>; PURPOSES],
}

const MAX_REDIRECTS: usize = 10;

impl Net {
    pub fn new(policy: Policy) -> Self {
        Self::with_hosts(policy, Hosts::default())
    }

    fn with_hosts(policy: Policy, hosts: Hosts) -> Self {
        Net {
            shared: Arc::new(Shared { policy: RwLock::new(policy), hosts, activity: Mutex::default() }),
            clients: Default::default(),
        }
    }

    /// A gate that lets plain http through to the given local hosts only, with
    /// every switch on.
    #[cfg(test)]
    pub fn for_tests(hosts: &[&str]) -> Self {
        Self::with_hosts(
            Policy::default(),
            Hosts { only: Some(hosts.iter().map(|h| h.to_string()).collect()), plain_http: true },
        )
    }

    pub fn policy(&self) -> Policy {
        self.shared.policy()
    }

    pub fn set_policy(&self, policy: Policy) {
        *self.shared.policy.write().unwrap_or_else(|e| e.into_inner()) = policy;
    }

    /// Why every request for `purpose` would be refused right now (offline
    /// mode or a switch), whatever its address. `None` when they may go out.
    pub fn refusal(&self, purpose: Purpose) -> Option<Reason> {
        let policy = self.policy();
        if policy.offline {
            Some(Reason::Offline)
        } else {
            policy.switched_off(purpose).map(Reason::SwitchedOff)
        }
    }

    pub fn snapshot(&self) -> Snapshot {
        let activity = self.shared.activity.lock().unwrap_or_else(|e| e.into_inner());
        Snapshot {
            policy: self.policy(),
            entries: activity.entries.iter().rev().cloned().collect(),
            counts: Purpose::ALL
                .iter()
                .map(|&purpose| PurposeCount { purpose, count: activity.counts[purpose as usize] })
                .filter(|c| c.count != Count::default())
                .collect(),
        }
    }

    pub fn get(&self, purpose: Purpose, url: impl AsRef<str>) -> Request<'_> {
        self.request(purpose, reqwest::Method::GET, url.as_ref())
    }

    pub fn request(&self, purpose: Purpose, method: reqwest::Method, url: &str) -> Request<'_> {
        let target = reqwest::Url::parse(url)
            .map(|u| (u.clone(), self.client(purpose).request(method, u)))
            .map_err(|e| format!("not a web address ({e})"));
        Request { net: self, purpose, target }
    }

    fn client(&self, purpose: Purpose) -> &reqwest::Client {
        self.clients[purpose as usize].get_or_init(|| build_client(purpose, self.shared.clone()))
    }
}

fn build_client(purpose: Purpose, shared: Arc<Shared>) -> reqwest::Client {
    let plain_http = shared.hosts.plain_http;
    let redirects = reqwest::redirect::Policy::custom(move |attempt| {
        let next = attempt.url().clone();
        let next_host = next.host_str().unwrap_or_default().to_string();
        let hops = attempt.previous().len();
        let from = attempt.previous().last().cloned();
        if let Some(from) = &from {
            shared.record(purpose, from, attempt.status().as_str(), format!("redirected to {next_host}"), false);
        }
        if hops >= MAX_REDIRECTS {
            shared.record(purpose, &next, "refused", "too many redirects", true);
            return attempt.error("too many redirects");
        }
        let from_host = from.as_ref().and_then(|u| u.host_str());
        if !crate::sources::curseforge::redirect_keeps_key_home(from_host, Some(&next_host)) {
            shared.record(purpose, &next, "refused", "would take the CurseForge key off CurseForge", true);
            return attempt.error("CurseForge redirected away from its API host");
        }
        match shared.decide(purpose, &next) {
            Ok(()) => attempt.follow(),
            Err(reason) => {
                shared.record(purpose, &next, "refused", reason.short(), true);
                attempt.error(Refused { purpose, host: next_host, reason })
            }
        }
    });
    let mut builder = reqwest::Client::builder()
        .user_agent(crate::USER_AGENT)
        .redirect(redirects)
        .timeout(Duration::from_secs(120))
        .connect_timeout(Duration::from_secs(15));
    if !plain_http {
        // Applies to every hop, so a redirect can't downgrade a download to plain http.
        builder = builder.https_only(true);
    }
    builder.build().expect("http client")
}

/// One request on its way out. Built like a `reqwest::RequestBuilder`;
/// [`Request::send`] asks the gate first.
pub struct Request<'a> {
    net: &'a Net,
    purpose: Purpose,
    target: Result<(reqwest::Url, reqwest::RequestBuilder), String>,
}

impl Request<'_> {
    fn map(mut self, f: impl FnOnce(reqwest::RequestBuilder) -> reqwest::RequestBuilder) -> Self {
        self.target = self.target.map(|(url, builder)| (url, f(builder)));
        self
    }

    pub fn header(self, name: &'static str, value: impl AsRef<str>) -> Self {
        self.map(|b| b.header(name, value.as_ref()))
    }

    pub fn timeout(self, timeout: Duration) -> Self {
        self.map(|b| b.timeout(timeout))
    }

    pub fn bearer_auth(self, token: &str) -> Self {
        self.map(|b| b.bearer_auth(token))
    }

    pub fn json<T: Serialize + ?Sized>(self, body: &T) -> Self {
        self.map(|b| b.json(body))
    }

    /// Send it, if the gate allows. A refusal comes back as a [`Refused`]
    /// error, whether it was the first address or a redirect.
    pub async fn send(self) -> anyhow::Result<reqwest::Response> {
        let (url, builder) = self.target.map_err(anyhow::Error::msg)?;
        let shared = &self.net.shared;
        if let Err(reason) = shared.decide(self.purpose, &url) {
            shared.record(self.purpose, &url, "refused", reason.short(), true);
            let host = url.host_str().unwrap_or_default().to_string();
            return Err(Refused { purpose: self.purpose, host, reason }.into());
        }
        match builder.send().await {
            Ok(resp) => {
                shared.record(self.purpose, resp.url(), resp.status().as_str(), "", false);
                Ok(resp)
            }
            Err(e) => {
                if let Some(refused) = refused_in(&e) {
                    // Recorded where the redirect was stopped.
                    return Err(refused.into());
                }
                if !e.is_redirect() {
                    shared.record(self.purpose, e.url().unwrap_or(&url), "failed", failure(&e), false);
                }
                Err(e.into())
            }
        }
    }
}

/// The gate's refusal inside a redirect error, if that is what stopped it.
fn refused_in(e: &reqwest::Error) -> Option<Refused> {
    let mut source = std::error::Error::source(e);
    while let Some(s) = source {
        if let Some(r) = s.downcast_ref::<Refused>() {
            return Some(r.clone());
        }
        source = s.source();
    }
    None
}

fn failure(e: &reqwest::Error) -> &'static str {
    if e.is_timeout() {
        "timed out"
    } else if e.is_connect() {
        "could not connect"
    } else {
        "connection error"
    }
}

// ---------------------------------------------------------------- reading replies

/// API replies: CurseForge search pages, GitHub release lists, Wago, WoWI and
/// Tukui details. The largest seen in practice is well under 1 MB.
pub const JSON_LIMIT: usize = 16 * 1024 * 1024;

/// README files, `.pkgmeta`, rendered README HTML.
pub const TEXT_LIMIT: usize = 2 * 1024 * 1024;

/// Read a response body, stopping as soon as it passes `limit` bytes.
pub async fn read_capped(mut resp: reqwest::Response, limit: usize) -> anyhow::Result<Vec<u8>> {
    if resp.content_length().is_some_and(|n| n > limit as u64) {
        anyhow::bail!("reply is larger than the {} limit", size_label(limit));
    }
    let mut out = Vec::new();
    while let Some(chunk) = resp.chunk().await? {
        if out.len() + chunk.len() > limit {
            anyhow::bail!("reply is larger than the {} limit", size_label(limit));
        }
        out.extend_from_slice(&chunk);
    }
    Ok(out)
}

/// A JSON reply, at most `JSON_LIMIT` bytes.
pub async fn json<T: DeserializeOwned>(resp: reqwest::Response) -> anyhow::Result<T> {
    let bytes = read_capped(resp, JSON_LIMIT).await?;
    Ok(serde_json::from_slice(&bytes)?)
}

/// A text reply, at most `limit` bytes. Invalid UTF-8 is replaced, as
/// `Response::text` does.
pub async fn text(resp: reqwest::Response, limit: usize) -> anyhow::Result<String> {
    let bytes = read_capped(resp, limit).await?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn size_label(limit: usize) -> String {
    if limit >= 1024 * 1024 {
        format!("{} MB", limit >> 20)
    } else {
        format!("{} KB", (limit >> 10).max(1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    /// One plain-HTTP reply from a local socket. Nothing leaves the machine.
    fn serve_once(body: Vec<u8>, declare_length: bool) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            let (mut sock, _) = listener.accept().unwrap();
            let mut buf = [0u8; 1024];
            let _ = sock.read(&mut buf);
            let head = if declare_length {
                format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len())
            } else {
                "HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n".to_string()
            };
            let _ = sock.write_all(head.as_bytes());
            let _ = sock.write_all(&body);
        });
        format!("http://{addr}/")
    }

    fn fetch(url: &str, limit: usize) -> anyhow::Result<Vec<u8>> {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        rt.block_on(async {
            let resp = reqwest::Client::new().get(url).send().await?;
            read_capped(resp, limit).await
        })
    }

    #[test]
    fn a_reply_within_the_limit_is_read_whole() {
        let url = serve_once(b"{\"ok\":true}".to_vec(), true);
        assert_eq!(fetch(&url, 1024).unwrap(), b"{\"ok\":true}");
    }

    #[test]
    fn a_declared_length_over_the_limit_is_refused_before_reading() {
        let url = serve_once(vec![b'x'; 4096], true);
        let err = fetch(&url, 1024).unwrap_err().to_string();
        assert!(err.contains("1 KB limit"), "{err}");
    }

    #[test]
    fn an_undeclared_length_is_cut_off_at_the_limit() {
        let url = serve_once(vec![b'x'; 64 * 1024], false);
        let err = fetch(&url, 1024).unwrap_err().to_string();
        assert!(err.contains("limit"), "{err}");
    }
}

#[cfg(test)]
mod policy_tests {
    use super::*;

    fn verdict(policy: &Policy, purpose: Purpose, url: &str) -> Result<(), Reason> {
        decide(policy, &Hosts::default(), purpose, &reqwest::Url::parse(url).unwrap())
    }

    fn allowed(purpose: Purpose, url: &str) -> bool {
        verdict(&Policy::default(), purpose, url).is_ok()
    }

    #[test]
    fn each_purpose_reaches_the_hosts_its_site_serves_from() {
        let cases = [
            (Purpose::Catalogue, "https://raw.githubusercontent.com/l3gitpanda/addonforge/main/catalog/forever.json"),
            (Purpose::Stats, "https://raw.githubusercontent.com/l3gitpanda/addonforge/main/catalog/stats.json"),
            (Purpose::GitHub, "https://api.github.com/repos/o/r/releases/latest"),
            (Purpose::GitHub, "https://github.com/o/r/releases/latest/download/release.json"),
            (Purpose::GitHub, "https://release-assets.githubusercontent.com/github-production-release-asset/1"),
            (Purpose::GitHub, "https://codeload.github.com/o/r/zip/refs/tags/v1"),
            (Purpose::GitHub, "https://raw.githubusercontent.com/o/r/v1/.pkgmeta"),
            (Purpose::CurseForge, "https://api.curseforge.com/v1/mods/1"),
            (Purpose::CurseForge, "https://edge.forgecdn.net/files/1/2/A.zip"),
            (Purpose::CurseForge, "https://mediafilez.forgecdn.net/files/1/2/A.zip"),
            (Purpose::WoWInterface, "https://api.mmoui.com/v3/game/WOW/filedetails/5086.json"),
            (Purpose::WoWInterface, "https://cdn.wowinterface.com/downloads/getfile.php?id=5086"),
            (Purpose::Tukui, "https://api.tukui.org/v1/addon/elvui"),
            (Purpose::Wago, "https://addons.wago.io/api/external/addons/x"),
            (Purpose::Details, "https://api.github.com/repos/o/r/readme"),
            (Purpose::Details, "https://raw.githubusercontent.com/o/r/HEAD/README.md"),
            (Purpose::Images, "https://github.com/owner.png?size=64"),
            (Purpose::Images, "https://avatars.githubusercontent.com/u/1"),
            (Purpose::Images, "https://camo.githubusercontent.com/abc"),
            (Purpose::Images, "https://media.forgecdn.net/avatars/1.png"),
            (Purpose::Images, "https://cdn-wow.mmoui.com/preview/1.jpg"),
        ];
        for (purpose, url) in cases {
            assert!(allowed(purpose, url), "{purpose:?} {url}");
        }
    }

    #[test]
    fn hosts_outside_a_purpose_are_refused() {
        let cases = [
            // Look-alikes.
            (Purpose::GitHub, "https://evilgithub.com/a.zip"),
            (Purpose::GitHub, "https://github.com.evil.example/a.zip"),
            (Purpose::GitHub, "https://notgithubusercontent.com/a.zip"),
            (Purpose::CurseForge, "https://api.curseforge.com.evil.example/v1/mods"),
            (Purpose::CurseForge, "https://forgecdn.net.evil.example/a.zip"),
            (Purpose::Tukui, "https://evil-tukui.org/a.zip"),
            // Another site's host.
            (Purpose::GitHub, "https://api.curseforge.com/v1/mods/1"),
            (Purpose::Tukui, "https://github.com/tukui-org/ElvUI/archive/a.zip"),
            (Purpose::WoWInterface, "https://evil.example/getfile.php"),
            // The catalogue and the details are pinned to their exact hosts.
            (Purpose::Catalogue, "https://github.com/l3gitpanda/addonforge/raw/main/catalog/forever.json"),
            (Purpose::Details, "https://evil.githubusercontent.com/README.md"),
            (Purpose::Images, "https://media.forgecdn.net.evil.example/a.png"),
            (Purpose::Images, "https://i.imgur.com/a.png"),
            // A user name in the address.
            (Purpose::GitHub, "https://github.com@evil.example/a.zip"),
            (Purpose::GitHub, "https://user:pass@github.com/a.zip"),
        ];
        for (purpose, url) in cases {
            assert_eq!(verdict(&Policy::default(), purpose, url), Err(Reason::Host), "{purpose:?} {url}");
        }
        assert_eq!(verdict(&Policy::default(), Purpose::GitHub, "http://github.com/a.zip"), Err(Reason::Scheme));
        assert_eq!(verdict(&Policy::default(), Purpose::Images, "ftp://github.com/a.png"), Err(Reason::Scheme));
    }

    #[test]
    fn offline_mode_refuses_everything_even_allowed_hosts() {
        let offline = Policy { offline: true, ..Policy::default() };
        for purpose in Purpose::ALL {
            let url = match purpose {
                Purpose::Catalogue | Purpose::Stats | Purpose::Details => "https://raw.githubusercontent.com/a",
                Purpose::CurseForge => "https://api.curseforge.com/v1/mods/1",
                Purpose::WoWInterface => "https://api.mmoui.com/a",
                Purpose::Tukui => "https://api.tukui.org/a",
                Purpose::Wago => "https://addons.wago.io/a",
                Purpose::AppUpdate | Purpose::GitHub | Purpose::Images => "https://github.com/a",
            };
            assert!(allowed(purpose, url), "{purpose:?} is allowed online");
            assert_eq!(verdict(&offline, purpose, url), Err(Reason::Offline), "{purpose:?}");
        }
    }

    #[test]
    fn a_switch_turns_off_its_source_and_that_sites_images() {
        let no_github = Policy { github: false, ..Policy::default() };
        assert_eq!(verdict(&no_github, Purpose::GitHub, "https://api.github.com/a"), Err(Reason::SwitchedOff("GitHub")));
        assert_eq!(verdict(&no_github, Purpose::Details, "https://api.github.com/a"), Err(Reason::SwitchedOff("GitHub")));
        assert_eq!(
            verdict(&no_github, Purpose::Images, "https://avatars.githubusercontent.com/u/1"),
            Err(Reason::SwitchedOff("GitHub"))
        );
        assert_eq!(verdict(&no_github, Purpose::Images, "https://media.forgecdn.net/a.png"), Ok(()), "other sites' images stay on");
        assert_eq!(verdict(&no_github, Purpose::CurseForge, "https://api.curseforge.com/v1/mods/1"), Ok(()));
        assert_eq!(
            verdict(&no_github, Purpose::Catalogue, "https://raw.githubusercontent.com/a"),
            Ok(()),
            "the catalogue has its own switch"
        );

        let no_curse = Policy { curseforge: false, ..Policy::default() };
        assert_eq!(
            verdict(&no_curse, Purpose::Images, "https://media.forgecdn.net/a.png"),
            Err(Reason::SwitchedOff("CurseForge"))
        );

        let no_images = Policy { images: false, ..Policy::default() };
        assert_eq!(verdict(&no_images, Purpose::Images, "https://github.com/a.png"), Err(Reason::SwitchedOff("Remote images")));
        assert_eq!(verdict(&no_images, Purpose::GitHub, "https://github.com/a.zip"), Ok(()));

        let no_catalogue = Policy { catalogue: false, ..Policy::default() };
        assert_eq!(
            verdict(&no_catalogue, Purpose::Catalogue, "https://raw.githubusercontent.com/a"),
            Err(Reason::SwitchedOff("Catalogue refresh"))
        );
        assert_eq!(verdict(&no_catalogue, Purpose::Stats, "https://raw.githubusercontent.com/a"), Ok(()));

        let everything_off = Policy {
            catalogue: false,
            stats: false,
            images: false,
            curseforge: false,
            github: false,
            wowinterface: false,
            tukui: false,
            ..Policy::default()
        };
        assert_eq!(verdict(&everything_off, Purpose::AppUpdate, "https://github.com/a"), Ok(()), "only offline mode stops it");
    }

    #[test]
    fn switches_are_set_by_name_and_summed_up() {
        let mut p = Policy::default();
        assert_eq!(p.summary(), "online, everything on");
        assert!(p.set("github", false) && p.set("launch-check", false));
        assert!(!p.set("gitlab", false), "no such switch");
        assert_eq!(p.summary(), "online; off: launch-check, github");
        assert!(p.set("offline", true));
        assert_eq!(p.summary(), "offline mode");
        // Every name reaches its own field: flip each away from its default.
        let mut every = Policy::default();
        for name in Policy::SWITCHES {
            assert!(every.set(name, name == "offline"), "{name}");
        }
        let flipped = Policy {
            offline: true,
            catalogue: false,
            stats: false,
            launch_check: false,
            images: false,
            curseforge: false,
            github: false,
            wowinterface: false,
            tukui: false,
        };
        assert_eq!(every, flipped);
    }

    #[test]
    fn older_settings_files_keep_every_switch_on() {
        let empty: Policy = serde_json::from_str("{}").unwrap();
        assert_eq!(empty, Policy::default());
        let offline: Policy = serde_json::from_str(r#"{"offline": true}"#).unwrap();
        assert!(offline.offline && offline.github && offline.launch_check);
    }

    #[test]
    fn a_refusal_names_the_host_and_what_to_do() {
        let r = Refused { purpose: Purpose::GitHub, host: "api.github.com".into(), reason: Reason::Offline };
        assert!(r.to_string().contains("did not contact api.github.com"), "{r}");
        let r = Refused { purpose: Purpose::WoWInterface, host: "evil.example".into(), reason: Reason::Host };
        assert!(r.to_string().starts_with("A WoWInterface request tried to reach evil.example"), "{r}");
        assert!(Reason::SwitchedOff("GitHub").not_checked().contains("GitHub is switched off"));
    }

    #[test]
    fn links_found_in_data_are_checked_without_the_switches() {
        assert!(allowed_host(Purpose::Images, "https://github.com/owner.png"));
        assert!(!allowed_host(Purpose::Images, "https://evil.example/a.png"));
        assert!(!allowed_host(Purpose::Images, "not a url"));
    }
}

#[cfg(test)]
mod gate_tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    /// A local server that answers each connection with the next reply in
    /// turn, and returns how many it served. `replies` gets the port, for
    /// replies that redirect back to it. It stops waiting after ten seconds,
    /// so a request that never comes fails the test instead of hanging it.
    fn serve(replies: impl FnOnce(u16) -> Vec<String>) -> (u16, std::thread::JoinHandle<usize>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let replies = replies(port);
        let handle = std::thread::spawn(move || {
            listener.set_nonblocking(true).unwrap();
            let deadline = std::time::Instant::now() + Duration::from_secs(10);
            let mut served = 0;
            for reply in replies {
                let mut sock = loop {
                    match listener.accept() {
                        Ok((sock, _)) => break sock,
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock && std::time::Instant::now() < deadline => {
                            std::thread::sleep(Duration::from_millis(10))
                        }
                        Err(_) => return served,
                    }
                };
                sock.set_nonblocking(false).unwrap();
                let mut buf = [0u8; 2048];
                let _ = sock.read(&mut buf);
                let _ = sock.write_all(reply.as_bytes());
                served += 1;
            }
            served
        });
        (port, handle)
    }

    fn ok(body: &str) -> String {
        format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len())
    }

    fn redirect(to: &str) -> String {
        format!("HTTP/1.1 302 Found\r\nLocation: {to}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
    }

    fn run<T>(f: impl std::future::Future<Output = T>) -> T {
        tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(f)
    }

    #[test]
    fn an_allowed_request_is_sent_and_listed() {
        let (port, server) = serve(|_| vec![ok("hello")]);
        let net = Net::for_tests(&["127.0.0.1"]);
        let resp = run(net.get(Purpose::GitHub, format!("http://127.0.0.1:{port}/repos/o/r?page=2")).send()).unwrap();
        assert_eq!(resp.status(), 200);
        assert_eq!(server.join().unwrap(), 1);
        let snap = net.snapshot();
        assert_eq!(snap.entries.len(), 1);
        let e = &snap.entries[0];
        assert_eq!((e.purpose, e.host.as_str(), e.path.as_str(), e.outcome.as_str()), (Purpose::GitHub, "127.0.0.1", "/repos/o/r", "200"));
        assert_eq!(snap.counts.len(), 1);
        assert_eq!(snap.counts[0].count, Count { sent: 1, refused: 0 });
    }

    #[test]
    fn offline_mode_opens_no_connection() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let net = Net::for_tests(&["127.0.0.1"]);
        net.set_policy(Policy { offline: true, ..Policy::default() });
        let err = run(net.get(Purpose::CurseForge, format!("http://127.0.0.1:{port}/v1/mods/1")).send()).unwrap_err();
        let refused = err.downcast_ref::<Refused>().expect("a refusal");
        assert_eq!(refused.reason, Reason::Offline);
        assert!(
            matches!(listener.accept(), Err(e) if e.kind() == std::io::ErrorKind::WouldBlock),
            "nothing connected to the server"
        );
        let snap = net.snapshot();
        assert_eq!(snap.entries[0].outcome, "refused");
        assert_eq!(snap.counts[0].count, Count { sent: 0, refused: 1 });
    }

    #[test]
    fn a_switched_off_source_is_refused_and_the_others_are_not() {
        let (port, server) = serve(|_| vec![ok("{}")]);
        let net = Net::for_tests(&["127.0.0.1"]);
        net.set_policy(Policy { github: false, ..Policy::default() });
        let err = run(net.get(Purpose::GitHub, format!("http://127.0.0.1:{port}/a")).send()).unwrap_err();
        assert_eq!(err.downcast_ref::<Refused>().map(|r| r.reason.clone()), Some(Reason::SwitchedOff("GitHub")));
        assert_eq!(net.refusal(Purpose::Details), Some(Reason::SwitchedOff("GitHub")));
        assert_eq!(net.refusal(Purpose::Tukui), None);
        run(net.get(Purpose::Tukui, format!("http://127.0.0.1:{port}/a")).send()).unwrap();
        assert_eq!(server.join().unwrap(), 1, "only the Tukui request reached the server");
    }

    #[test]
    fn a_redirect_to_a_host_the_purpose_may_not_use_is_not_followed() {
        // `localhost` is the same machine under another name: the gate goes by
        // the name, so the hop is stopped before any connection to it.
        let (port, server) = serve(|port| vec![redirect(&format!("http://localhost:{port}/elsewhere.zip"))]);
        let net = Net::for_tests(&["127.0.0.1"]);
        let err = run(net.get(Purpose::WoWInterface, format!("http://127.0.0.1:{port}/getfile.php?id=1")).send()).unwrap_err();
        let refused = err.downcast_ref::<Refused>().expect("a refusal");
        assert_eq!((refused.host.as_str(), &refused.reason), ("localhost", &Reason::Host));
        assert!(refused.to_string().starts_with("A WoWInterface request tried to reach localhost"), "{refused}");
        assert_eq!(server.join().unwrap(), 1, "only the first address was contacted");
        let snap = net.snapshot();
        let outcomes: Vec<(&str, &str, &str)> =
            snap.entries.iter().rev().map(|e| (e.host.as_str(), e.outcome.as_str(), e.detail.as_str())).collect();
        assert_eq!(
            outcomes,
            [("127.0.0.1", "302", "redirected to localhost"), ("localhost", "refused", "host not allowed")]
        );
        assert_eq!(snap.counts[0].count, Count { sent: 1, refused: 1 });
    }

    #[test]
    fn a_redirect_within_the_allowed_hosts_is_followed() {
        let (port, server) =
            serve(|port| vec![redirect(&format!("http://127.0.0.1:{port}/files/A.zip")), ok("zip bytes")]);
        let net = Net::for_tests(&["127.0.0.1"]);
        let resp = run(net.get(Purpose::GitHub, format!("http://127.0.0.1:{port}/download/A.zip")).send()).unwrap();
        assert_eq!(resp.status(), 200);
        assert_eq!(resp.url().path(), "/files/A.zip");
        assert_eq!(server.join().unwrap(), 2);
        let paths: Vec<(String, String)> =
            net.snapshot().entries.iter().rev().map(|e| (e.path.clone(), e.outcome.clone())).collect();
        assert_eq!(paths, [("/download/A.zip".to_string(), "302".to_string()), ("/files/A.zip".to_string(), "200".to_string())]);
    }

    #[test]
    fn a_change_of_settings_applies_to_the_next_request() {
        let (port, server) = serve(|_| vec![ok("{}")]);
        let net = Net::for_tests(&["127.0.0.1"]);
        net.set_policy(Policy { offline: true, ..Policy::default() });
        assert!(run(net.get(Purpose::Stats, format!("http://127.0.0.1:{port}/a")).send()).is_err());
        net.set_policy(Policy::default());
        run(net.get(Purpose::Stats, format!("http://127.0.0.1:{port}/a")).send()).unwrap();
        assert_eq!(server.join().unwrap(), 1);
    }
}
