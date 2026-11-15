//! `forge publish ios`: over-the-air installs for development builds.
//!
//! The laptop builds and signs the app into an `.ipa` (no phone needed),
//! writes the install manifest iOS expects, and serves both over HTTPS on
//! the tailnet with `tailscale serve`. On the iPhone, a page lists every
//! published app with an Install button: an `itms-services://` link that
//! iOS follows to download and install the build, wherever the phone is.
//!
//! Proven on Cati (iOS 26.3.1) with a free Apple team on 2026-11-14: the
//! development profile `xtool dev build --sign --ipa` makes lists the phone,
//! and iOS installs from the link as an update to the installed app.
//!
//! The files live in one folder (`FORGE_PUBLISH_DIR`, default
//! `~/.local/share/forge/publish`). A small file server, `forge publish
//! serve`, serves that folder on 127.0.0.1 only; `tailscale serve` makes it
//! reachable over HTTPS from devices on the tailnet and nowhere else.

use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::runner::{Cmd, CommandRunner};

/// The tailnet HTTPS port. Not 443: a laptop often serves something else
/// there, and `tailscale serve --bg` would replace it.
pub const DEFAULT_HTTPS_PORT: u16 = 8443;

/// How many builds of each app the page keeps by default.
pub const DEFAULT_KEEP: usize = 5;

/// The local file server's port (127.0.0.1 only).
pub const LOCAL_PORT: u16 = 47411;

/// Answered by `forge publish serve` with the folder it serves, so a publish
/// can tell its own server from something else on the port.
const PROBE_PATH: &str = "/.forge-publish";

const APPS_JSON: &str = "apps.json";
const INDEX_HTML: &str = "index.html";
const PID_FILE: &str = "server.pid";
const SERVER_LOG: &str = "server.log";

/// What Forge needs to know about a signed `.ipa`.
#[derive(Debug, Clone, PartialEq)]
pub struct IpaInfo {
    pub name: String,
    /// As signed: free-team builds are `XTL-<team>.<id>`.
    pub bundle_id: String,
    pub version: String,
    pub build: String,
    /// When the provisioning profile expires (seconds since 1970); the app
    /// stops opening then.
    pub expires: Option<u64>,
    /// The device UDIDs the profile allows.
    pub devices: Vec<String>,
}

/// One published build, as `apps.json` lists it (newest first).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    /// Unique per app: `<published>-<commit>`. A folder and URL segment.
    #[serde(default)]
    pub id: String,
    pub name: String,
    pub bundle_id: String,
    pub version: String,
    pub build: String,
    /// Seconds since 1970.
    pub published: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
    /// The branch or `--label`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Whether `<bundle>/<id>/icon.png` exists.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub icon: bool,
    pub size: u64,
}

/// The folder Forge publishes into.
pub fn publish_dir(env: &dyn Fn(&str) -> Option<String>) -> PathBuf {
    if let Some(dir) = env("FORGE_PUBLISH_DIR").filter(|v| !v.is_empty()) {
        return PathBuf::from(dir);
    }
    let data = env("XDG_DATA_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env("HOME").unwrap_or_default()).join(".local/share"));
    data.join("forge/publish")
}

// ---------------------------------------------------------------------------
// Reading the .ipa

/// Reads the app's Info.plist and embedded provisioning profile.
pub fn read_ipa(path: &Path) -> Result<IpaInfo, String> {
    let file = fs::File::open(path).map_err(|e| format!("cannot open {}: {e}", path.display()))?;
    let mut zip = zip::ZipArchive::new(file)
        .map_err(|e| format!("{} is not an .ipa (zip): {e}", path.display()))?;
    let names: Vec<String> = (0..zip.len())
        .filter_map(|i| {
            zip.by_index(i)
                .ok()
                .and_then(|f| f.name().ok().map(|n| n.into_owned()))
        })
        .collect();
    let app_file = |leaf: &str| {
        names
            .iter()
            .find(|n| {
                n.starts_with("Payload/")
                    && n.matches('/').count() == 2
                    && n.split('/').nth(1).is_some_and(|a| a.ends_with(".app"))
                    && n.ends_with(&format!("/{leaf}"))
            })
            .cloned()
    };
    let read = |zip: &mut zip::ZipArchive<fs::File>, name: &str| -> Result<Vec<u8>, String> {
        let mut entry = zip
            .by_name(name)
            .map_err(|e| format!("cannot read {name} in the .ipa: {e}"))?;
        let mut bytes = Vec::new();
        entry
            .read_to_end(&mut bytes)
            .map_err(|e| format!("cannot read {name} in the .ipa: {e}"))?;
        Ok(bytes)
    };
    let info_name = app_file("Info.plist")
        .ok_or_else(|| format!("{} has no Payload/<App>.app/Info.plist", path.display()))?;
    let info = read(&mut zip, &info_name)?;
    let profile = match app_file("embedded.mobileprovision") {
        Some(name) => Some(read(&mut zip, &name)?),
        None => None,
    };
    parse_ipa(&info, profile.as_deref())
}

/// The app's icon from the `.ipa`, for the page and the install prompt: the
/// largest `AppIcon*.png` at the top of the app. `None` when the app has no
/// icon, or only Xcode's crushed PNGs (CgBI), which browsers can't show.
pub fn ipa_icon(path: &Path) -> Option<Vec<u8>> {
    let file = fs::File::open(path).ok()?;
    let mut zip = zip::ZipArchive::new(file).ok()?;
    let mut best: Option<(u64, usize)> = None;
    for i in 0..zip.len() {
        let Ok(f) = zip.by_index(i) else { continue };
        let Some(name) = f.name().ok().map(|n| n.into_owned()) else {
            continue;
        };
        let parts: Vec<&str> = name.split('/').collect();
        let is_icon = parts.len() == 3
            && parts[0] == "Payload"
            && parts[1].ends_with(".app")
            && parts[2].starts_with("AppIcon")
            && parts[2].ends_with(".png");
        if is_icon && best.is_none_or(|(size, _)| f.size() > size) {
            best = Some((f.size(), i));
        }
    }
    let (size, index) = best?;
    if size > 4 * 1024 * 1024 {
        return None;
    }
    let mut bytes = Vec::new();
    zip.by_index(index).ok()?.read_to_end(&mut bytes).ok()?;
    let png = bytes.starts_with(b"\x89PNG\r\n\x1a\n");
    let crushed = bytes.get(12..16) == Some(b"CgBI");
    (png && !crushed).then_some(bytes)
}

fn parse_ipa(info: &[u8], profile: Option<&[u8]>) -> Result<IpaInfo, String> {
    let info = plist::Value::from_reader(std::io::Cursor::new(info))
        .map_err(|e| format!("the app's Info.plist is unreadable: {e}"))?;
    let info = info
        .as_dictionary()
        .ok_or("the app's Info.plist is not a dictionary")?;
    let text = |key: &str| info.get(key).and_then(plist::Value::as_string);
    let bundle_id = text("CFBundleIdentifier")
        .ok_or("the app's Info.plist has no CFBundleIdentifier")?
        .to_owned();
    let name = text("CFBundleDisplayName")
        .or_else(|| text("CFBundleName"))
        .unwrap_or(&bundle_id)
        .to_owned();
    let version = text("CFBundleShortVersionString")
        .unwrap_or("1.0")
        .to_owned();
    let build = text("CFBundleVersion").unwrap_or(&version).to_owned();

    let (mut expires, mut devices) = (None, Vec::new());
    if let Some(profile) = profile.and_then(profile_plist) {
        if let Some(dict) = profile.as_dictionary() {
            expires = dict
                .get("ExpirationDate")
                .and_then(plist::Value::as_date)
                .map(SystemTime::from)
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_secs());
            devices = dict
                .get("ProvisionedDevices")
                .and_then(plist::Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_string().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_default();
        }
    }
    Ok(IpaInfo {
        name,
        bundle_id,
        version,
        build,
        expires,
        devices,
    })
}

/// The XML plist inside a signed (CMS) `.mobileprovision`.
fn profile_plist(bytes: &[u8]) -> Option<plist::Value> {
    let start = find(bytes, b"<?xml")?;
    let end = find(&bytes[start..], b"</plist>")? + start + b"</plist>".len();
    plist::Value::from_reader_xml(&bytes[start..end]).ok()
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

// ---------------------------------------------------------------------------
// The build stamp

/// What a published build says about itself in its Info.plist, so the app
/// (and anyone holding the phone) can tell which build is installed. Read on
/// the phone with ForgeKit's `ForgeBuildInfo`.
#[derive(Debug, Clone, PartialEq)]
pub struct Stamp {
    /// CFBundleVersion: the app's build number, then the minute it was built
    /// (`1.29911107`), so every build has a new, higher number and iOS
    /// always treats an install as an update.
    pub build: String,
    /// `ForgeBuildCommit`: the short commit, with `+changes` when the
    /// checkout had uncommitted changes.
    pub commit: Option<String>,
    /// `ForgeBuildBranch`.
    pub branch: Option<String>,
    /// `ForgeBuildDate`: `2026-11-14 14:57 UTC`.
    pub date: String,
}

impl Stamp {
    pub fn new(
        build_number: &str,
        now: u64,
        commit: Option<String>,
        branch: Option<String>,
    ) -> Self {
        let base = build_number
            .split('.')
            .next()
            .filter(|b| !b.is_empty() && b.chars().all(|c| c.is_ascii_digit()))
            .unwrap_or("1");
        Stamp {
            build: format!("{base}.{}", now / 60),
            commit,
            branch,
            date: when(now),
        }
    }
}

/// `info` (an Info.plist) with the stamp's keys set.
pub fn stamp_info_plist(info: &[u8], stamp: &Stamp) -> Result<Vec<u8>, String> {
    let mut value = plist::Value::from_reader(std::io::Cursor::new(info))
        .map_err(|e| format!("cannot read Info.plist: {e}"))?;
    let dict = value
        .as_dictionary_mut()
        .ok_or("Info.plist is not a dictionary")?;
    dict.insert("CFBundleVersion".into(), stamp.build.clone().into());
    dict.insert("ForgeBuildDate".into(), stamp.date.clone().into());
    for (key, v) in [
        ("ForgeBuildCommit", &stamp.commit),
        ("ForgeBuildBranch", &stamp.branch),
    ] {
        match v {
            Some(v) => {
                dict.insert(key.into(), v.clone().into());
            }
            None => {
                dict.remove(key);
            }
        }
    }
    let mut out = Vec::new();
    value
        .to_writer_xml(&mut out)
        .map_err(|e| format!("cannot write Info.plist: {e}"))?;
    // Keep the comments above `<plist>` (Forge's "Generated by Forge" marker),
    // so that if this build is killed before the original is put back, the
    // next build still knows the file is Forge's and regenerates it.
    let original = String::from_utf8_lossy(info);
    let head = original.split("<plist").next().unwrap_or("");
    let comments: String = head
        .lines()
        .map(str::trim)
        .filter(|l| l.starts_with("<!--"))
        .map(|l| format!("{l}\n"))
        .collect();
    if comments.is_empty() {
        return Ok(out);
    }
    let text = String::from_utf8(out).map_err(|e| format!("cannot write Info.plist: {e}"))?;
    Ok(match text.find("<plist") {
        Some(at) => format!("{}{comments}{}", &text[..at], &text[at..]),
        None => text,
    }
    .into_bytes())
}

// ---------------------------------------------------------------------------
// The published folder

/// A bundle id is safe as a folder and URL path segment.
fn safe_id(bundle_id: &str) -> Result<(), String> {
    let ok = !bundle_id.is_empty()
        && !bundle_id.starts_with('.')
        && bundle_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_');
    if ok {
        Ok(())
    } else {
        Err(format!("unexpected bundle id {bundle_id:?}"))
    }
}

pub fn page_url(base: &str) -> String {
    format!("{}/", base.trim_end_matches('/'))
}

/// A build id is safe as a folder and URL path segment.
fn safe_build(id: &str) -> bool {
    !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

/// `1794664323-8a8c56a`: when it was published, then its commit.
fn build_id(now: u64, commit: Option<&str>) -> String {
    let commit: String = commit
        .unwrap_or("")
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .take(12)
        .collect();
    if commit.is_empty() {
        now.to_string()
    } else {
        format!("{now}-{commit}")
    }
}

pub fn manifest_url(base: &str, entry: &Entry) -> String {
    format!(
        "{}/{}/{}/manifest.plist",
        base.trim_end_matches('/'),
        entry.bundle_id,
        entry.id
    )
}

/// The link iOS follows to install this build (Safari asks first).
pub fn install_url(base: &str, entry: &Entry) -> String {
    format!(
        "itms-services://?action=download-manifest&url={}",
        manifest_url(base, entry)
    )
}

/// The manifest iOS downloads before the `.ipa`.
pub fn manifest(base: &str, entry: &Entry) -> Result<Vec<u8>, String> {
    use plist::{Dictionary, Value};
    let mut asset = Dictionary::new();
    asset.insert("kind".into(), "software-package".into());
    asset.insert(
        "url".into(),
        format!(
            "{}/{}/{}/app.ipa",
            base.trim_end_matches('/'),
            entry.bundle_id,
            entry.id
        )
        .into(),
    );
    let mut assets = vec![Value::Dictionary(asset)];
    if entry.icon {
        // Shown on the Home Screen while the app downloads.
        for kind in ["display-image", "full-size-image"] {
            let mut image = Dictionary::new();
            image.insert("kind".into(), kind.into());
            image.insert("needs-shine".into(), false.into());
            image.insert(
                "url".into(),
                format!(
                    "{}/{}/{}/icon.png",
                    base.trim_end_matches('/'),
                    entry.bundle_id,
                    entry.id
                )
                .into(),
            );
            assets.push(Value::Dictionary(image));
        }
    }
    let mut metadata = Dictionary::new();
    metadata.insert("bundle-identifier".into(), entry.bundle_id.clone().into());
    metadata.insert("bundle-version".into(), entry.version.clone().into());
    metadata.insert("kind".into(), "software".into());
    metadata.insert("title".into(), entry.name.clone().into());
    let mut item = Dictionary::new();
    item.insert("assets".into(), Value::Array(assets));
    item.insert("metadata".into(), Value::Dictionary(metadata));
    let mut root = Dictionary::new();
    root.insert("items".into(), Value::Array(vec![Value::Dictionary(item)]));
    let mut out = Vec::new();
    Value::Dictionary(root)
        .to_writer_xml(&mut out)
        .map_err(|e| format!("cannot write the manifest: {e}"))?;
    Ok(out)
}

/// The builds in `apps.json`, newest first. Entries from before builds had
/// ids (one build per app) are dropped.
fn read_entries(dir: &Path) -> Vec<Entry> {
    let mut entries: Vec<Entry> = fs::read(dir.join(APPS_JSON))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();
    entries.retain(|e| safe_build(&e.id) && safe_id(&e.bundle_id).is_ok());
    entries.sort_by_key(|e| std::cmp::Reverse(e.published));
    entries
}

/// Writes `bytes` to `path` through a temporary file, so the server never
/// hands out half a file.
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let tmp = path.with_extension("partial");
    fs::write(&tmp, bytes).map_err(|e| format!("cannot write {}: {e}", tmp.display()))?;
    fs::rename(&tmp, path).map_err(|e| format!("cannot write {}: {e}", path.display()))
}

/// Adds the `.ipa` as a new build, keeps the newest `keep` builds of each
/// app (deleting older ones), and rewrites `apps.json` and the page for
/// `base` (the HTTPS address the phone uses). Earlier builds stay
/// installable: a publish never replaces another.
pub fn stage(
    dir: &Path,
    ipa: &Path,
    info: &IpaInfo,
    base: &str,
    source: Source,
    now: u64,
    keep: usize,
) -> Result<Entry, String> {
    safe_id(&info.bundle_id)?;
    let mut entries = read_entries(dir);
    let mut id = build_id(now, source.commit.as_deref());
    // Two publishes in the same second from the same commit.
    let mut n = 1;
    while entries
        .iter()
        .any(|e| e.bundle_id == info.bundle_id && e.id == id)
    {
        n += 1;
        id = format!("{}-{n}", build_id(now, source.commit.as_deref()));
    }
    let build_dir = dir.join(&info.bundle_id).join(&id);
    fs::create_dir_all(&build_dir)
        .map_err(|e| format!("cannot create {}: {e}", build_dir.display()))?;
    let tmp = build_dir.join("app.ipa.partial");
    let size = fs::copy(ipa, &tmp).map_err(|e| format!("cannot copy {}: {e}", ipa.display()))?;
    fs::rename(&tmp, build_dir.join("app.ipa"))
        .map_err(|e| format!("cannot publish the .ipa: {e}"))?;
    let icon = ipa_icon(ipa).is_some_and(|png| fs::write(build_dir.join("icon.png"), png).is_ok());
    let entry = Entry {
        id,
        name: info.name.clone(),
        bundle_id: info.bundle_id.clone(),
        version: info.version.clone(),
        build: info.build.clone(),
        published: now,
        expires: info.expires,
        commit: source.commit,
        label: source.label,
        size,
        icon,
    };
    entries.insert(0, entry.clone());
    entries.sort_by_key(|e| std::cmp::Reverse(e.published));

    // Keep the newest `keep` builds of each app.
    let mut seen: Vec<(String, usize)> = Vec::new();
    let mut kept = Vec::new();
    for e in entries {
        let count = match seen.iter_mut().find(|(b, _)| *b == e.bundle_id) {
            Some((_, n)) => {
                *n += 1;
                *n
            }
            None => {
                seen.push((e.bundle_id.clone(), 1));
                1
            }
        };
        if count <= keep.max(1) {
            kept.push(e);
        } else {
            let _ = fs::remove_dir_all(dir.join(&e.bundle_id).join(&e.id));
        }
    }
    rewrite(dir, &kept, base)?;
    Ok(entry)
}

/// Where a build came from: shown on the page so builds from different
/// branches are easy to tell apart.
#[derive(Debug, Clone, Default)]
pub struct Source {
    pub commit: Option<String>,
    /// The branch, or what `--label` says (a PR, a feature).
    pub label: Option<String>,
}

fn rewrite(dir: &Path, entries: &[Entry], base: &str) -> Result<(), String> {
    for e in entries {
        let build_dir = dir.join(&e.bundle_id).join(&e.id);
        if build_dir.is_dir() {
            write_atomic(&build_dir.join("manifest.plist"), &manifest(base, e)?)?;
        }
    }
    let json = serde_json::to_vec_pretty(entries).map_err(|e| e.to_string())?;
    write_atomic(&dir.join(APPS_JSON), &json)?;
    write_atomic(&dir.join(INDEX_HTML), index_html(base, entries).as_bytes())
}

/// `2026-11-21 13:24 UTC`.
fn when(secs: u64) -> String {
    let s = crate::report::stamp_at(secs); // 2026-11-21T13-24-47
    format!("{} {}:{} UTC", &s[..10], &s[11..13], &s[14..16])
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// `52.5 MB`.
fn size_text(bytes: u64) -> String {
    if bytes == 0 {
        String::new()
    } else if bytes < 1024 * 1024 {
        format!("{} KB", bytes.div_ceil(1024))
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

/// A colour for an app without an icon, the same for the same app every time.
fn app_hue(bundle_id: &str) -> u32 {
    bundle_id
        .bytes()
        .fold(7u32, |h, b| h.wrapping_mul(31).wrapping_add(u32::from(b)))
        % 360
}

/// The app's icon, or its initial on a coloured tile.
fn icon_html(base: &str, e: &Entry, class: &str) -> String {
    if e.icon {
        format!(
            r#"<img class="{class}" src="{}/{}/{}/icon.png" alt="">"#,
            esc(base.trim_end_matches('/')),
            esc(&e.bundle_id),
            esc(&e.id)
        )
    } else {
        let hue = app_hue(&e.bundle_id);
        let initial: String = e
            .name
            .chars()
            .find(|c| c.is_alphanumeric())
            .into_iter()
            .collect();
        format!(
            r#"<div class="{class} mono" style="--h:{hue}" aria-hidden="true">{}</div>"#,
            esc(&initial.to_uppercase())
        )
    }
}

/// Where a build came from: its label (branch, PR) and commit.
fn source_chips(e: &Entry) -> String {
    let mut chips = String::new();
    if let Some(label) = &e.label {
        chips.push_str(&format!(
            r#"<span class="chip label">{}</span>"#,
            esc(label)
        ));
    }
    if let Some(commit) = &e.commit {
        chips.push_str(&format!(
            r#"<span class="chip commit">{}</span>"#,
            esc(commit)
        ));
    }
    if chips.is_empty() {
        chips.push_str(r#"<span class="chip label">Build</span>"#);
    }
    chips
}

/// When it was published (made relative on the phone) and its size.
fn facts(e: &Entry) -> String {
    let mut out = format!(
        r#"<time data-ago="{}">{}</time>"#,
        e.published,
        when(e.published)
    );
    let size = size_text(e.size);
    if !size.is_empty() {
        out.push_str(&format!(" · {size}"));
    }
    out
}

fn expiry_html(e: &Entry) -> String {
    e.expires
        .map(|x| {
            format!(
                r#"<p class="expires" data-expires="{x}">Signed until {}</p>"#,
                when(x)
            )
        })
        .unwrap_or_default()
}

fn install_link(base: &str, e: &Entry, class: &str, what: &str) -> String {
    let title = e
        .label
        .as_deref()
        .or(e.commit.as_deref())
        .unwrap_or("build");
    format!(
        r#"<a class="{class}" href="{}" data-testid="install.{}.{}" aria-label="Install {} {what}, {}">Install</a>"#,
        esc(&install_url(base, e)),
        esc(&e.bundle_id),
        esc(&e.id),
        esc(&e.name),
        esc(title),
    )
}

/// One app: its newest build up front, earlier ones folded away.
fn app_html(base: &str, builds: &[&Entry]) -> String {
    let e = builds[0];
    let mut out = format!(
        r#"<article class="app" data-name="{name_lower}">
<div class="head">
{icon}
<div class="title">
<h2>{name}</h2>
<p class="version">Version {version} ({build})</p>
</div>
{install}
</div>
<div class="chips">{chips}</div>
<p class="facts">{facts}</p>
{expiry}"#,
        name_lower = esc(&e.name.to_lowercase()),
        icon = icon_html(base, e, "icon"),
        name = esc(&e.name),
        version = esc(&e.version),
        build = esc(&e.build),
        install = install_link(base, e, "get", "newest build"),
        chips = source_chips(e),
        facts = facts(e),
        expiry = expiry_html(e),
    );
    if builds.len() > 1 {
        out.push_str(&format!(
            "<details><summary>{} earlier build{}</summary>\n<ul>\n",
            builds.len() - 1,
            if builds.len() == 2 { "" } else { "s" }
        ));
        for b in &builds[1..] {
            out.push_str(&format!(
                r#"<li><div><div class="chips">{}</div><p class="facts">Version {} ({}) · {}</p>{}</div>{}</li>
"#,
                source_chips(b),
                esc(&b.version),
                esc(&b.build),
                facts(b),
                expiry_html(b),
                install_link(base, b, "again", "earlier build")
            ));
        }
        out.push_str("</ul>\n</details>\n");
    }
    out.push_str("</article>\n");
    out
}

/// The page the phone opens: each app (the most recently published first)
/// with its builds, newest first.
pub fn index_html(base: &str, entries: &[Entry]) -> String {
    let mut apps: Vec<(&str, Vec<&Entry>)> = Vec::new();
    for e in entries {
        match apps.iter_mut().find(|(b, _)| *b == e.bundle_id) {
            Some((_, builds)) => builds.push(e),
            None => apps.push((&e.bundle_id, vec![e])),
        }
    }
    let mut cards = String::new();
    for (_, builds) in &apps {
        cards.push_str(&app_html(base, builds));
    }
    let summary = match (apps.len(), entries.first()) {
        (0, _) | (_, None) => String::new(),
        (n, Some(latest)) => format!(
            r#"{n} app{} · last published <time data-ago="{}">{}</time>"#,
            if n == 1 { "" } else { "s" },
            latest.published,
            when(latest.published)
        ),
    };
    if cards.is_empty() {
        cards.push_str(
            r#"<div class="empty"><p>Nothing published yet.</p><p>On the laptop: <code>forge publish ios --project &lt;app&gt;</code></p></div>"#,
        );
    }
    let search = if apps.len() > 4 {
        r#"<input id="filter" type="search" placeholder="Find an app" aria-label="Find an app" data-testid="install.filter" autocomplete="off">"#
    } else {
        ""
    };
    format!(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1, viewport-fit=cover">
<meta name="apple-mobile-web-app-title" content="Forge">
<title>Forge · Install</title>
<style>
:root {{ color-scheme: light dark; --bg: #f2f2f7; --card: #fff; --text: #1c1c1e; --muted: #6c6c70; --accent: #007aff; --chip: #eef0f4; --warn: #c93400; --bad: #d70015; --line: #e5e5ea; --shadow: 0 1px 2px rgba(0,0,0,.06), 0 4px 16px rgba(0,0,0,.05); }}
@media (prefers-color-scheme: dark) {{ :root {{ --bg: #000; --card: #1c1c1e; --text: #f2f2f7; --muted: #98989f; --accent: #0a84ff; --chip: #2c2c2e; --warn: #ff9f0a; --bad: #ff453a; --line: #38383a; --shadow: none; }} }}
* {{ box-sizing: border-box; }}
body {{ margin: 0; background: var(--bg); color: var(--text); font: 17px/1.4 -apple-system, system-ui, sans-serif; -webkit-text-size-adjust: 100%; }}
.wrap {{ max-width: 640px; margin: 0 auto; padding: max(20px, env(safe-area-inset-top)) 16px max(32px, env(safe-area-inset-bottom)); }}
header {{ margin: 12px 4px 20px; }}
.brand {{ font-size: 13px; font-weight: 600; letter-spacing: .06em; text-transform: uppercase; color: var(--accent); margin: 0; }}
h1 {{ font-size: 34px; line-height: 1.1; margin: 2px 0 4px; letter-spacing: -.01em; }}
.sub {{ color: var(--muted); margin: 0; font-size: 15px; }}
#filter {{ width: 100%; margin-top: 14px; padding: 10px 14px; border: 0; border-radius: 10px; background: var(--chip); color: var(--text); font: inherit; }}
.app {{ background: var(--card); border-radius: 18px; padding: 16px; margin: 0 0 14px; box-shadow: var(--shadow); }}
.head {{ display: flex; align-items: center; gap: 14px; }}
.icon {{ width: 64px; height: 64px; border-radius: 15px; flex: none; object-fit: cover; box-shadow: inset 0 0 0 .5px rgba(0,0,0,.12); }}
.mono {{ display: grid; place-items: center; color: #fff; font-size: 30px; font-weight: 700; background: linear-gradient(145deg, hsl(var(--h) 75% 58%), hsl(calc(var(--h) + 40) 70% 42%)); }}
.title {{ flex: 1; min-width: 0; }}
h2 {{ margin: 0; font-size: 20px; line-height: 1.2; overflow-wrap: anywhere; }}
.version {{ margin: 2px 0 0; color: var(--muted); font-size: 13px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }}
.get {{ flex: none; padding: 7px 20px; border-radius: 999px; background: var(--accent); color: #fff; font-weight: 700; font-size: 15px; text-decoration: none; }}
.get:active, .again:active {{ opacity: .6; }}
.chips {{ display: flex; flex-wrap: wrap; gap: 6px; margin: 12px 0 0; }}
.chip {{ font-size: 13px; padding: 2px 9px; border-radius: 999px; background: var(--chip); color: var(--text); max-width: 100%; overflow-wrap: anywhere; }}
.chip.commit {{ font-family: ui-monospace, Menlo, monospace; font-size: 12px; color: var(--muted); }}
.facts {{ margin: 8px 0 0; color: var(--muted); font-size: 14px; }}
.expires {{ margin: 4px 0 0; color: var(--muted); font-size: 14px; }}
.expires.soon {{ color: var(--warn); font-weight: 600; }}
.expires.gone {{ color: var(--bad); font-weight: 600; }}
details {{ margin-top: 12px; border-top: 1px solid var(--line); }}
summary {{ color: var(--accent); padding: 12px 0 2px; cursor: pointer; font-size: 15px; list-style: none; }}
summary::-webkit-details-marker {{ display: none; }}
summary::after {{ content: ""; display: inline-block; width: 7px; height: 7px; margin: 0 0 3px 8px; border: solid currentColor; border-width: 0 2px 2px 0; transform: rotate(45deg); transition: transform .2s; }}
details[open] summary::after {{ transform: rotate(-135deg); margin-bottom: -1px; }}
ul {{ list-style: none; margin: 0; padding: 0; }}
li {{ display: flex; align-items: center; gap: 12px; padding: 12px 0; border-bottom: 1px solid var(--line); }}
li:last-child {{ border-bottom: none; padding-bottom: 0; }}
li > div {{ flex: 1; min-width: 0; }}
li .chips {{ margin: 0; }}
li .facts {{ margin-top: 4px; }}
.again {{ flex: none; padding: 5px 14px; border-radius: 999px; border: 1.5px solid var(--accent); color: var(--accent); font-weight: 600; font-size: 14px; text-decoration: none; }}
.empty {{ background: var(--card); border-radius: 18px; padding: 24px 16px; text-align: center; color: var(--muted); }}
code {{ font-family: ui-monospace, Menlo, monospace; font-size: 14px; }}
footer {{ margin: 24px 4px 0; color: var(--muted); font-size: 13px; }}
footer p {{ margin: 0 0 8px; }}
</style>
</head>
<body>
<div class="wrap">
<header>
<p class="brand">Forge</p>
<h1>Install</h1>
<p class="sub">{summary}</p>
{search}
</header>
<main>
{cards}</main>
<footer>
<p>Tap Install, then Install again when the phone asks. The app downloads on the Home Screen.</p>
<p>A new build replaces the app and keeps its data. To go back, install an earlier build the same way.</p>
<p>Free signing lasts 7 days: publishing again renews it.</p>
</footer>
</div>
<script>
(() => {{
  const now = Date.now() / 1000;
  const day = 86400;
  const ago = s => {{
    const d = now - s;
    if (d < 60) return "just now";
    if (d < 3600) return Math.floor(d / 60) + " min ago";
    if (d < day) return Math.floor(d / 3600) + " h ago";
    if (d < 2 * day) return "yesterday";
    return Math.floor(d / day) + " days ago";
  }};
  for (const t of document.querySelectorAll("time[data-ago]")) {{
    t.title = t.textContent;
    t.textContent = ago(Number(t.dataset.ago));
  }}
  for (const p of document.querySelectorAll("[data-expires]")) {{
    const left = Number(p.dataset.expires) - now;
    if (left <= 0) {{ p.textContent = "Signature expired: publish it again"; p.classList.add("gone"); }}
    else if (left < day) {{ p.textContent = "Expires in " + Math.max(1, Math.round(left / 3600)) + " h"; p.classList.add("soon"); }}
    else {{ const n = Math.floor(left / day); p.textContent = "Expires in " + n + " day" + (n === 1 ? "" : "s"); if (n < 2) p.classList.add("soon"); }}
  }}
  const f = document.getElementById("filter");
  if (f) f.addEventListener("input", () => {{
    const q = f.value.trim().toLowerCase();
    for (const a of document.querySelectorAll(".app")) a.hidden = !!q && !a.dataset.name.includes(q);
  }});
}})();
</script>
</body>
</html>
"#
    )
}

// ---------------------------------------------------------------------------
// The file server

fn content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("plist") => "text/xml",
        Some("json") => "application/json",
        Some("ipa") => "application/octet-stream",
        Some("png") => "image/png",
        _ => "application/octet-stream",
    }
}

/// The file `url_path` names inside `dir`. Only the page, `apps.json` and
/// each app's manifest and `.ipa` are served: never the server's pid or log.
fn resolve(dir: &Path, url_path: &str) -> Option<PathBuf> {
    let path = url_path.split(['?', '#']).next().unwrap_or("");
    let parts: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    match parts.as_slice() {
        [""] | [INDEX_HTML] => Some(dir.join(INDEX_HTML)),
        [APPS_JSON] => Some(dir.join(APPS_JSON)),
        [id, build, file @ ("manifest.plist" | "app.ipa" | "icon.png")]
            if safe_id(id).is_ok() && safe_build(build) =>
        {
            Some(dir.join(id).join(build).join(file))
        }
        _ => None,
    }
}

/// What the probe answers: a fingerprint of the folder, so the folder's
/// path (and the user name in it) never leaves the computer.
fn folder_token(dir: &Path) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(dir.to_string_lossy().as_bytes());
    digest.iter().take(12).map(|b| format!("{b:02x}")).collect()
}

fn respond(stream: &mut TcpStream, status: &str, kind: &str, body: &[u8], head: bool) {
    let _ = write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    );
    if !head {
        let _ = stream.write_all(body);
    }
}

fn handle(mut stream: TcpStream, dir: &Path) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
    let mut reader = BufReader::new(match stream.try_clone() {
        Ok(s) => s,
        Err(_) => return,
    });
    let mut line = String::new();
    if reader.read_line(&mut line).is_err() {
        return;
    }
    // Keep only Range: iOS can resume a large .ipa download with it.
    let mut range = None;
    let mut header = String::new();
    while reader.read_line(&mut header).is_ok_and(|n| n > 2) {
        if let Some((name, value)) = header.split_once(':') {
            if name.trim().eq_ignore_ascii_case("range") {
                range = Some(value.trim().to_string());
            }
        }
        header.clear();
    }
    let mut parts = line.split_whitespace();
    let (method, target) = (parts.next().unwrap_or(""), parts.next().unwrap_or("/"));
    let head = method == "HEAD";
    if method != "GET" && !head {
        return respond(
            &mut stream,
            "405 Method Not Allowed",
            "text/plain",
            b"",
            head,
        );
    }
    if target == PROBE_PATH {
        let body = folder_token(dir);
        return respond(&mut stream, "200 OK", "text/plain", body.as_bytes(), head);
    }
    let Some(path) = resolve(dir, target) else {
        return respond(
            &mut stream,
            "404 Not Found",
            "text/plain",
            b"not found",
            head,
        );
    };
    let Ok(mut file) = fs::File::open(&path) else {
        return respond(
            &mut stream,
            "404 Not Found",
            "text/plain",
            b"not found",
            head,
        );
    };
    let len = file.metadata().map(|m| m.len()).unwrap_or(0);
    let kind = content_type(&path);
    let (status, start, end) = match range.as_deref().map(|r| byte_range(r, len)) {
        None => ("200 OK", 0, len),
        Some(Some((start, end))) => ("206 Partial Content", start, end),
        Some(None) => {
            let _ = write!(
                stream,
                "HTTP/1.1 416 Range Not Satisfiable\r\nContent-Range: bytes */{len}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            );
            return;
        }
    };
    let content_range = if status.starts_with("206") {
        format!("Content-Range: bytes {start}-{}/{len}\r\n", end - 1)
    } else {
        String::new()
    };
    let _ = write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\n{content_range}Accept-Ranges: bytes\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        end - start
    );
    if !head {
        use std::io::Seek;
        if file.seek(std::io::SeekFrom::Start(start)).is_ok() {
            let _ = std::io::copy(&mut (&mut file).take(end - start), &mut stream);
        }
    }
}

/// `bytes=N-`, `bytes=N-M` or `bytes=-N` as `[start, end)` within `len`
/// bytes; None if it can't be served (several ranges, or past the end).
fn byte_range(header: &str, len: u64) -> Option<(u64, u64)> {
    let spec = header.strip_prefix("bytes=")?.trim();
    if spec.contains(',') {
        return None;
    }
    let (a, b) = spec.split_once('-')?;
    let (start, end) = match (a.trim(), b.trim()) {
        ("", n) => {
            let n: u64 = n.parse().ok()?;
            (len.saturating_sub(n), len)
        }
        (a, "") => (a.parse().ok()?, len),
        (a, b) => (
            a.parse().ok()?,
            b.parse::<u64>().ok()?.saturating_add(1).min(len),
        ),
    };
    (start < end && start < len).then_some((start, end))
}

/// `forge publish serve`: serves `dir` on 127.0.0.1:`port` until stopped.
pub fn serve(dir: &Path, port: u16) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    let listener = TcpListener::bind(("127.0.0.1", port))
        .map_err(|e| format!("cannot listen on 127.0.0.1:{port}: {e}"))?;
    let _ = fs::write(dir.join(PID_FILE), std::process::id().to_string());
    println!(
        "[forge] Serving {} on http://127.0.0.1:{port}",
        dir.display()
    );
    for stream in listener.incoming().flatten() {
        let dir = dir.to_path_buf();
        std::thread::spawn(move || handle(stream, &dir));
    }
    Ok(())
}

/// The [`folder_token`] of the folder the server on `port` serves, if a
/// Forge server answers there.
fn probe(port: u16) -> Option<String> {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let mut s = TcpStream::connect_timeout(&addr, Duration::from_millis(500)).ok()?;
    let _ = s.set_read_timeout(Some(Duration::from_secs(2)));
    write!(s, "GET {PROBE_PATH} HTTP/1.1\r\nHost: localhost\r\n\r\n").ok()?;
    let mut out = String::new();
    s.read_to_string(&mut out).ok()?;
    let (head, body) = out.split_once("\r\n\r\n")?;
    head.starts_with("HTTP/1.1 200").then(|| body.to_owned())
}

/// Starts `forge publish serve` in the background unless it already runs.
fn ensure_server(dir: &Path, port: u16) -> Result<(), String> {
    match probe(port) {
        Some(served) if served == folder_token(dir) => return Ok(()),
        Some(_) => {
            return Err(format!(
                "another Forge publish server on 127.0.0.1:{port} serves a different folder: stop it with `forge publish stop` from the same FORGE_PUBLISH_DIR"
            ))
        }
        None => {}
    }
    if TcpStream::connect_timeout(
        &SocketAddr::from(([127, 0, 0, 1], port)),
        Duration::from_millis(300),
    )
    .is_ok()
    {
        return Err(format!(
            "something else is listening on 127.0.0.1:{port}, which Forge uses to serve published builds: stop it"
        ));
    }
    let exe = std::env::current_exe().map_err(|e| format!("cannot find the forge binary: {e}"))?;
    let log = fs::File::create(dir.join(SERVER_LOG))
        .map_err(|e| format!("cannot write {}: {e}", dir.join(SERVER_LOG).display()))?;
    let mut cmd = std::process::Command::new(exe);
    cmd.args(["publish", "serve", "--port", &port.to_string()])
        .env("FORGE_PUBLISH_DIR", dir)
        .stdin(std::process::Stdio::null())
        .stdout(log.try_clone().map_err(|e| e.to_string())?)
        .stderr(log);
    // Its own process group, so Ctrl-C in this terminal leaves it running.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    cmd.spawn()
        .map_err(|e| format!("cannot start the publish server: {e}"))?;
    for _ in 0..30 {
        if probe(port).is_some() {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Err(format!(
        "the publish server did not start: see {}",
        dir.join(SERVER_LOG).display()
    ))
}

// ---------------------------------------------------------------------------
// Tailscale

const TAILSCALE: &str = "tailscale";

/// `https://<laptop>.<tailnet>.ts.net:<port>` from `tailscale status`.
pub fn tailscale_base(runner: &dyn CommandRunner, https_port: u16) -> Result<String, String> {
    let out = runner
        .capture(&Cmd::new(TAILSCALE).args(["status", "--json"]))
        .map_err(|_| {
            "Tailscale is not installed on this computer. Install it (https://tailscale.com/download/linux), run `sudo tailscale up`, and install the Tailscale app on the iPhone (docs/over-the-air.md)".to_string()
        })?;
    let status: serde_json::Value = serde_json::from_slice(&out.stdout).map_err(|_| {
        "`tailscale status --json` printed no status: is tailscaled running? (`sudo systemctl start tailscaled`)".to_string()
    })?;
    if status["BackendState"].as_str() != Some("Running") {
        return Err(format!(
            "Tailscale is {} on this computer: run `sudo tailscale up`",
            status["BackendState"].as_str().unwrap_or("not connected")
        ));
    }
    let host = status["Self"]["DNSName"]
        .as_str()
        .unwrap_or("")
        .trim_end_matches('.');
    if host.is_empty() {
        return Err("this computer has no tailnet name: turn on MagicDNS and HTTPS certificates in the Tailscale admin console (DNS page)".into());
    }
    Ok(if https_port == 443 {
        format!("https://{host}")
    } else {
        format!("https://{host}:{https_port}")
    })
}

/// What `https_port` on the tailnet is proxied to now, from `tailscale serve
/// status --json` (background and foreground serves), if anything.
fn serving_on(status: &serde_json::Value, https_port: u16) -> Option<String> {
    let suffix = format!(":{https_port}");
    let from_web = |web: &serde_json::Value| -> Option<String> {
        web.as_object()?.iter().find_map(|(host, cfg)| {
            if !host.ends_with(&suffix) {
                return None;
            }
            let handlers = cfg["Handlers"].as_object()?;
            Some(
                handlers
                    .values()
                    .map(|h| {
                        h["Proxy"]
                            .as_str()
                            .or(h["Path"].as_str())
                            .or(h["Text"].as_str().map(|_| "text"))
                            .unwrap_or("something")
                            .to_string()
                    })
                    .collect::<Vec<_>>()
                    .join(", "),
            )
        })
    };
    if let Some(found) = from_web(&status["Web"]) {
        return Some(found);
    }
    if let Some(sessions) = status["Foreground"].as_object() {
        for session in sessions.values() {
            if let Some(found) = from_web(&session["Web"]) {
                return Some(found);
            }
        }
    }
    let tcp = |cfg: &serde_json::Value| cfg["TCP"].get(https_port.to_string()).is_some();
    if tcp(status)
        || status["Foreground"]
            .as_object()
            .is_some_and(|s| s.values().any(tcp))
    {
        return Some("something".into());
    }
    None
}

/// Whether `https_port` is free or already Forge's, or else what holds it.
fn port_owner(runner: &dyn CommandRunner, https_port: u16) -> Result<Option<String>, String> {
    let out = runner
        .capture(&Cmd::new(TAILSCALE).args(["serve", "status", "--json"]))
        .map_err(|e| format!("cannot read `tailscale serve status`: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "`tailscale serve status --json` failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    let text = String::from_utf8_lossy(&out.stdout);
    if text.trim().is_empty() {
        return Ok(None);
    }
    let status: serde_json::Value = serde_json::from_str(&text)
        .map_err(|e| format!("cannot read `tailscale serve status --json`: {e}"))?;
    Ok(serving_on(&status, https_port))
}

fn ours(target: &str) -> bool {
    target.trim_end_matches('/') == format!("http://127.0.0.1:{LOCAL_PORT}")
}

/// Points `https_port` on the tailnet at the local server, unless the port
/// already serves something else (which `--bg` would silently replace).
fn tailscale_serve(
    runner: &dyn CommandRunner,
    https_port: u16,
    local_port: u16,
) -> Result<(), String> {
    match port_owner(runner, https_port)? {
        Some(target) if ours(&target) => return Ok(()),
        Some(target) => {
            return Err(format!(
                "port {https_port} on your tailnet already serves {target}, and Forge won't replace it: publish on another port with --port (for example --port 9443)"
            ))
        }
        None => {}
    }
    // `timeout`: when Serve isn't enabled for the tailnet, tailscale prints a
    // link to enable it and waits.
    let cmd = Cmd::new("timeout").args([
        "60".to_string(),
        TAILSCALE.to_string(),
        "serve".into(),
        "--bg".into(),
        format!("--https={https_port}"),
        format!("http://127.0.0.1:{local_port}"),
    ]);
    let out = runner.capture(&cmd)?;
    if out.status.success() {
        return Ok(());
    }
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    Err(explain_serve(&text, out.status.code(), https_port))
}

fn explain_serve(output: &str, code: Option<i32>, https_port: u16) -> String {
    let fix = if output.contains("Access denied") || output.contains("must be root") {
        "let your user configure Tailscale once: `sudo tailscale set --operator=$USER`".to_string()
    } else if output.contains("listener already exists") {
        format!("port {https_port} is already served by another `tailscale serve`: pick another with --port")
    } else if code == Some(124) || output.contains("not enabled") || output.contains("To enable") {
        "turn on HTTPS certificates (and Serve, if asked) for your tailnet in the Tailscale admin console, then run this again".to_string()
    } else {
        "read tailscale's message above".to_string()
    };
    format!(
        "`tailscale serve` could not publish port {https_port}: {fix}\n{}",
        output.trim()
    )
}

// ---------------------------------------------------------------------------
// Keeping the server up

const UNIT: &str = "forge-publish.service";

/// The systemd user unit that runs `forge publish serve` at boot.
fn unit_text(exe: &Path, dir: &Path, port: u16) -> String {
    format!(
        "[Unit]\nDescription=Forge over-the-air builds (forge publish serve)\nAfter=network-online.target\n\n[Service]\nExecStart={} publish serve --port {port}\nEnvironment=FORGE_PUBLISH_DIR={}\nRestart=on-failure\n\n[Install]\nWantedBy=default.target\n",
        exe.display(),
        dir.display()
    )
}

/// `forge publish enable`: runs the server as a systemd user service, so the
/// Install page survives a reboot (`tailscale serve --bg` already does).
pub fn enable(
    runner: &dyn CommandRunner,
    dir: &Path,
    env: &dyn Fn(&str) -> Option<String>,
) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| format!("cannot find the forge binary: {e}"))?;
    if exe.components().any(|c| c.as_os_str() == "target") {
        println!(
            "[forge] Note: the service runs {}, a build inside a checkout. Install forge (`cargo install --path crates/forge-cli`) and run this again from the installed one if that checkout may go away.",
            exe.display()
        );
    }
    let config = env("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env("HOME").unwrap_or_default()).join(".config"));
    let units = config.join("systemd/user");
    fs::create_dir_all(&units).map_err(|e| format!("cannot create {}: {e}", units.display()))?;
    fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    fs::write(units.join(UNIT), unit_text(&exe, dir, LOCAL_PORT))
        .map_err(|e| format!("cannot write the service: {e}"))?;
    // A server `publish ios` started by hand holds the port: hand it over.
    if probe(LOCAL_PORT).as_deref() == Some(folder_token(dir).as_str()) {
        if let Ok(pid) = fs::read_to_string(dir.join(PID_FILE)) {
            let pid = pid.trim();
            if !pid.is_empty() && pid.chars().all(|c| c.is_ascii_digit()) {
                let _ = runner.capture(&Cmd::new("kill").arg(pid));
                std::thread::sleep(Duration::from_millis(300));
            }
        }
    }
    for args in [
        vec!["--user", "daemon-reload"],
        vec!["--user", "enable", "--now", UNIT],
    ] {
        let out = runner.capture(&Cmd::new("systemctl").args(&args))?;
        if !out.status.success() {
            return Err(format!(
                "`systemctl {}` failed: {}",
                args.join(" "),
                String::from_utf8_lossy(&out.stderr).trim()
            ));
        }
    }
    // Without lingering, user services stop when the last session ends.
    let user = env("USER").unwrap_or_default();
    let linger = runner.capture(&Cmd::new("loginctl").args(["enable-linger", &user]));
    if !linger.is_ok_and(|o| o.status.success()) {
        println!(
            "[forge] Run `sudo loginctl enable-linger {user}` once, so the service also runs when nobody is logged in."
        );
    }
    println!("[forge] The publish server now starts with this computer ({UNIT}). `systemctl --user disable --now {UNIT}` turns it off.");
    Ok(())
}

// ---------------------------------------------------------------------------
// Commands

/// What `forge publish ios` did.
pub struct Published {
    pub entry: Entry,
    pub page: String,
    pub install: String,
    pub devices: Vec<String>,
}

/// Publishes a signed `.ipa`: stages it, makes sure the server and
/// `tailscale serve` run, and returns the addresses.
pub fn publish(
    runner: &dyn CommandRunner,
    dir: &Path,
    ipa: &Path,
    https_port: u16,
    base_url: Option<String>,
    source: Source,
    keep: usize,
) -> Result<Published, String> {
    let info = read_ipa(ipa)?;
    let custom = base_url.is_some();
    let base = match base_url {
        Some(url) => url.trim_end_matches('/').to_string(),
        None => tailscale_base(runner, https_port)?,
    };
    // Before anything is staged: refuses a port another serve uses. With
    // --url, whatever serves that address is the person's own setup.
    if !custom {
        tailscale_serve(runner, https_port, LOCAL_PORT)?;
    }
    fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let entry = stage(dir, ipa, &info, &base, source, now, keep)?;
    ensure_server(dir, LOCAL_PORT)?;
    Ok(Published {
        page: page_url(&base),
        install: install_url(&base, &entry),
        entry,
        devices: info.devices,
    })
}

/// `forge publish stop`: stops serving on the tailnet and the local server.
/// Published files stay.
/// Leaves a port that serves anything other than Forge's server alone.
pub fn stop(runner: &dyn CommandRunner, dir: &Path, https_port: u16) -> Result<(), String> {
    match port_owner(runner, https_port) {
        Ok(Some(target)) if ours(&target) => {
            let _ = runner.capture(&Cmd::new(TAILSCALE).args([
                "serve".to_string(),
                format!("--https={https_port}"),
                "off".into(),
            ]));
        }
        Ok(Some(target)) => println!(
            "[forge] Left port {https_port} alone: it serves {target}, not Forge (pass the --port you published on)"
        ),
        Ok(None) | Err(_) => {}
    }
    if let Ok(pid) = fs::read_to_string(dir.join(PID_FILE)) {
        let pid = pid.trim();
        if !pid.is_empty()
            && pid.chars().all(|c| c.is_ascii_digit())
            && probe(LOCAL_PORT).as_deref() == Some(folder_token(dir).as_str())
        {
            let _ = runner.capture(&Cmd::new("kill").arg(pid));
        }
        let _ = fs::remove_file(dir.join(PID_FILE));
    }
    println!(
        "[forge] Stopped publishing. The builds stay in {}",
        dir.display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const INFO: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>XTL-ND4P87PX52.org.forgemobile.devicelab</string>
<key>CFBundleDisplayName</key><string>Device Lab</string>
<key>CFBundleShortVersionString</key><string>0.3</string>
<key>CFBundleVersion</key><string>7</string>
</dict></plist>"#;

    const PROFILE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
<key>ExpirationDate</key><date>2026-11-21T13:24:47Z</date>
<key>ProvisionedDevices</key><array><string>00008130-00164D583646001C</string></array>
</dict></plist>"#;

    fn signed_profile() -> Vec<u8> {
        // A CMS envelope around the XML, as Apple ships it.
        let mut bytes = vec![0x30, 0x80, 0x06, 0x09];
        bytes.extend_from_slice(PROFILE.as_bytes());
        bytes.extend_from_slice(&[0xa0, 0x82, 0x0b]);
        bytes
    }

    fn write_ipa(path: &Path) {
        let mut zip = zip::ZipWriter::new(fs::File::create(path).unwrap());
        let opts = zip::write::SimpleFileOptions::default();
        zip.start_file("Payload/AppHost.app/Info.plist", opts)
            .unwrap();
        zip.write_all(INFO.as_bytes()).unwrap();
        zip.start_file(
            "Payload/AppHost.app/Frameworks/X.framework/Info.plist",
            opts,
        )
        .unwrap();
        zip.write_all(b"not this one").unwrap();
        zip.start_file("Payload/AppHost.app/embedded.mobileprovision", opts)
            .unwrap();
        zip.write_all(&signed_profile()).unwrap();
        zip.finish().unwrap();
    }

    #[test]
    fn the_icon_reaches_the_page_and_the_install_prompt() {
        let tmp = tempfile::tempdir().unwrap();
        let ipa = tmp.path().join("App.ipa");
        let png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDRbig";
        {
            let mut zip = zip::ZipWriter::new(fs::File::create(&ipa).unwrap());
            let opts = zip::write::SimpleFileOptions::default();
            zip.start_file("Payload/AppHost.app/Info.plist", opts)
                .unwrap();
            zip.write_all(INFO.as_bytes()).unwrap();
            zip.start_file("Payload/AppHost.app/AppIcon60x60@2x.png", opts)
                .unwrap();
            zip.write_all(&png[..12]).unwrap();
            zip.start_file("Payload/AppHost.app/AppIcon76x76@2x~ipad.png", opts)
                .unwrap();
            zip.write_all(png).unwrap();
            zip.start_file("Payload/AppHost.app/Frameworks/AppIconX.png", opts)
                .unwrap();
            zip.write_all(&[png.as_slice(), &[0; 64]].concat()).unwrap();
            zip.finish().unwrap();
        }
        assert_eq!(
            ipa_icon(&ipa).as_deref(),
            Some(png.as_slice()),
            "the largest top-level AppIcon"
        );

        let dir = tmp.path().join("pub");
        let info = read_ipa(&ipa).unwrap();
        let e = stage(&dir, &ipa, &info, "https://h", Source::default(), 1, 5).unwrap();
        assert!(e.icon);
        assert!(
            resolve(&dir, &format!("/{}/{}/icon.png", e.bundle_id, e.id))
                .unwrap()
                .is_file()
        );
        let manifest =
            fs::read_to_string(dir.join(&e.bundle_id).join(&e.id).join("manifest.plist")).unwrap();
        assert!(
            manifest.contains("display-image") && manifest.contains("/icon.png"),
            "{manifest}"
        );
        let page = fs::read_to_string(dir.join("index.html")).unwrap();
        assert!(
            page.contains(r#"<img class="icon" src="https://h/"#),
            "{page}"
        );

        // No icon: a coloured initial instead, and no image in the manifest.
        write_ipa(&ipa);
        assert!(ipa_icon(&ipa).is_none());
        let e = stage(&dir, &ipa, &info, "https://h", Source::default(), 2, 5).unwrap();
        assert!(!e.icon);
        assert!(index_html("https://h", &[e]).contains(r#"class="icon mono""#));
    }

    #[test]
    fn reads_the_app_and_its_profile_from_an_ipa() {
        let tmp = tempfile::tempdir().unwrap();
        let ipa = tmp.path().join("App.ipa");
        write_ipa(&ipa);
        let info = read_ipa(&ipa).unwrap();
        assert_eq!(info.bundle_id, "XTL-ND4P87PX52.org.forgemobile.devicelab");
        assert_eq!(info.name, "Device Lab");
        assert_eq!((info.version.as_str(), info.build.as_str()), ("0.3", "7"));
        assert_eq!(info.devices, ["00008130-00164D583646001C"]);
        assert_eq!(when(info.expires.unwrap()), "2026-11-21 13:24 UTC");
    }

    #[test]
    fn a_missing_profile_is_not_an_error() {
        let info = parse_ipa(INFO.as_bytes(), None).unwrap();
        assert_eq!(info.expires, None);
        assert!(info.devices.is_empty());
    }

    #[test]
    fn the_manifest_names_the_signed_bundle_id_and_the_build() {
        let entry = Entry {
            id: "100-abc1234".into(),
            name: "Device Lab".into(),
            bundle_id: "XTL-T.org.x".into(),
            version: "0.3".into(),
            build: "7".into(),
            published: 100,
            expires: None,
            commit: Some("abc1234".into()),
            label: None,
            size: 1,
            icon: false,
        };
        let xml =
            String::from_utf8(manifest("https://create.t.ts.net:8443/", &entry).unwrap()).unwrap();
        assert!(
            xml.contains(
                "<string>https://create.t.ts.net:8443/XTL-T.org.x/100-abc1234/app.ipa</string>"
            ),
            "{xml}"
        );
        assert!(xml.contains("<string>XTL-T.org.x</string>"));
        assert!(xml.contains("<string>software-package</string>"));
        assert_eq!(
            install_url("https://h.ts.net:8443", &entry),
            "itms-services://?action=download-manifest&url=https://h.ts.net:8443/XTL-T.org.x/100-abc1234/manifest.plist"
        );
    }

    fn source(commit: &str, label: &str) -> Source {
        Source {
            commit: Some(commit.into()).filter(|c: &String| !c.is_empty()),
            label: Some(label.into()).filter(|l: &String| !l.is_empty()),
        }
    }

    #[test]
    fn builds_never_replace_each_other() {
        let tmp = tempfile::tempdir().unwrap();
        let ipa = tmp.path().join("App.ipa");
        write_ipa(&ipa);
        let dir = tmp.path().join("pub");
        let base = "https://h.ts.net:8443";
        let mut info = read_ipa(&ipa).unwrap();
        let summaries = stage(
            &dir,
            &ipa,
            &info,
            base,
            source("aaa1111", "summaries"),
            10,
            5,
        )
        .unwrap();
        let ota = stage(&dir, &ipa, &info, base, source("bbb2222", "ota"), 20, 5).unwrap();
        // Same second, same commit: still a separate build.
        let again = stage(&dir, &ipa, &info, base, source("bbb2222", "ota"), 20, 5).unwrap();
        assert_ne!(again.id, ota.id);
        info.bundle_id = "XTL-T.org.other".into();
        info.name = "Other".into();
        stage(&dir, &ipa, &info, base, Source::default(), 15, 5).unwrap();

        let entries = read_entries(&dir);
        let order: Vec<_> = entries
            .iter()
            .map(|e| (e.name.as_str(), e.published))
            .collect();
        assert_eq!(
            order,
            [
                ("Device Lab", 20),
                ("Device Lab", 20),
                ("Other", 15),
                ("Device Lab", 10)
            ]
        );
        for e in [&summaries, &ota, &again] {
            let build = dir.join(&e.bundle_id).join(&e.id);
            assert!(build.join("app.ipa").is_file() && build.join("manifest.plist").is_file());
        }
        let page = fs::read_to_string(dir.join("index.html")).unwrap();
        // Apps in order of their newest build; each app's builds newest first.
        assert!(page.find("<h2>Device Lab").unwrap() < page.find("<h2>Other").unwrap());
        assert_eq!(page.matches("<h2>Device Lab").count(), 1);
        assert!(page.find(">ota").unwrap() < page.find(">summaries").unwrap());
        assert!(page.contains("2 earlier builds"));
        assert!(page.contains("aaa1111"));
        assert!(page.contains(&esc(&install_url(base, &summaries))));
        assert!(page.contains("Signed until 2026-11-21 13:24 UTC"));
    }

    #[test]
    fn keeps_the_newest_builds_of_each_app() {
        let tmp = tempfile::tempdir().unwrap();
        let ipa = tmp.path().join("App.ipa");
        write_ipa(&ipa);
        let dir = tmp.path().join("pub");
        let info = read_ipa(&ipa).unwrap();
        let first = stage(&dir, &ipa, &info, "https://h", Source::default(), 1, 2).unwrap();
        stage(&dir, &ipa, &info, "https://h", Source::default(), 2, 2).unwrap();
        stage(&dir, &ipa, &info, "https://h", Source::default(), 3, 2).unwrap();
        let ids: Vec<_> = read_entries(&dir).into_iter().map(|e| e.id).collect();
        assert_eq!(ids, ["3", "2"]);
        assert!(!dir.join(&first.bundle_id).join(&first.id).exists());
    }

    #[test]
    fn entries_from_before_build_ids_are_dropped() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(
            tmp.path().join(APPS_JSON),
            r#"[{"name":"Old","bundle_id":"XTL-T.org.x","version":"1","build":"1","published":1,"size":1}]"#,
        )
        .unwrap();
        assert!(read_entries(tmp.path()).is_empty());
    }

    #[test]
    fn unsafe_bundle_ids_are_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let ipa = tmp.path().join("App.ipa");
        write_ipa(&ipa);
        let mut info = read_ipa(&ipa).unwrap();
        info.bundle_id = "../escape".into();
        assert!(stage(
            tmp.path(),
            &ipa,
            &info,
            "https://h",
            Source::default(),
            1,
            5
        )
        .is_err());
    }

    #[test]
    fn every_build_gets_a_new_higher_build_number_and_its_source() {
        let a = Stamp::new(
            "1",
            1_794_664_323,
            Some("cf57f4e".into()),
            Some("main".into()),
        );
        let b = Stamp::new("1", 1_794_664_323 + 60, None, None);
        assert_eq!(a.build, "1.29911072");
        assert!(b.build > a.build);
        assert_eq!(Stamp::new("7.2.1", 60, None, None).build, "7.1");
        assert_eq!(Stamp::new("x", 60, None, None).build, "1.1");

        let stamped = stamp_info_plist(INFO.as_bytes(), &a).unwrap();
        let info = parse_ipa(&stamped, None).unwrap();
        assert_eq!(info.build, "1.29911072");
        assert_eq!(info.version, "0.3");
        let text = String::from_utf8(stamped).unwrap();
        assert!(
            text.contains("<key>ForgeBuildCommit</key>\n\t<string>cf57f4e</string>"),
            "{text}"
        );
        assert!(text.contains("<key>ForgeBuildBranch</key>\n\t<string>main</string>"));
        assert!(text.contains("<key>ForgeBuildDate</key>"));
        // A build without git info drops stale keys.
        let again = String::from_utf8(stamp_info_plist(text.as_bytes(), &b).unwrap()).unwrap();
        assert!(!again.contains("ForgeBuildCommit"));
    }

    #[test]
    fn byte_ranges() {
        assert_eq!(byte_range("bytes=10-", 100), Some((10, 100)));
        assert_eq!(byte_range("bytes=10-19", 100), Some((10, 20)));
        assert_eq!(byte_range("bytes=90-200", 100), Some((90, 100)));
        assert_eq!(byte_range("bytes=-30", 100), Some((70, 100)));
        assert_eq!(byte_range("bytes=100-", 100), None);
        assert_eq!(byte_range("bytes=0-1,5-6", 100), None);
        assert_eq!(byte_range("items=0-1", 100), None);
    }

    #[test]
    fn the_service_runs_the_server_for_this_folder() {
        let unit = unit_text(
            Path::new("/home/red/.cargo/bin/forge"),
            Path::new("/p"),
            47411,
        );
        assert!(unit.contains("ExecStart=/home/red/.cargo/bin/forge publish serve --port 47411"));
        assert!(unit.contains("Environment=FORGE_PUBLISH_DIR=/p"));
        assert!(unit.contains("WantedBy=default.target"));
    }

    #[test]
    fn the_server_never_leaves_its_folder() {
        let dir = Path::new("/srv/pub");
        assert_eq!(
            resolve(dir, "/a/1-b/app.ipa?v=3"),
            Some(dir.join("a/1-b/app.ipa"))
        );
        assert_eq!(
            resolve(dir, "/a/1-b/manifest.plist"),
            Some(dir.join("a/1-b/manifest.plist"))
        );
        assert_eq!(resolve(dir, "/a/app.ipa"), None);
        assert_eq!(resolve(dir, "/a/../app.ipa"), None);
        assert_eq!(resolve(dir, "/"), Some(dir.join("index.html")));
        assert_eq!(resolve(dir, "/apps.json"), Some(dir.join("apps.json")));
        assert_eq!(resolve(dir, "/../etc/passwd"), None);
        assert_eq!(resolve(dir, "/a/../../x"), None);
        assert_eq!(resolve(dir, "/../app.ipa"), None);
        assert_eq!(resolve(dir, "/.forge-secret"), None);
        assert_eq!(resolve(dir, "/server.pid"), None);
        assert_eq!(resolve(dir, "/server.log"), None);
        assert_eq!(resolve(dir, "/a/other.txt"), None);
    }

    #[test]
    fn the_server_serves_files_and_answers_the_probe() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_path_buf();
        fs::write(dir.join("index.html"), "<p>hi</p>").unwrap();
        fs::create_dir_all(dir.join("a/1-b")).unwrap();
        fs::write(dir.join("a/1-b/manifest.plist"), "<plist/>").unwrap();
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let served = dir.clone();
        std::thread::spawn(move || {
            for s in listener.incoming().flatten() {
                handle(s, &served);
            }
        });
        let get = |path: &str| {
            let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
            write!(s, "GET {path} HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
            let mut out = String::new();
            s.read_to_string(&mut out).unwrap();
            out
        };
        let root = get("/");
        assert!(
            root.starts_with("HTTP/1.1 200") && root.ends_with("<p>hi</p>"),
            "{root}"
        );
        let manifest = get("/a/1-b/manifest.plist");
        assert!(manifest.contains("Content-Type: text/xml"), "{manifest}");
        assert!(get("/missing").starts_with("HTTP/1.1 404"));
        assert!(get("/../x").starts_with("HTTP/1.1 404"));
        assert!(get("/server.pid").starts_with("HTTP/1.1 404"));
        let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
        write!(s, "GET / HTTP/1.1\r\nHost: x\r\nRange: bytes=3-\r\n\r\n").unwrap();
        let mut partial = String::new();
        s.read_to_string(&mut partial).unwrap();
        assert!(partial.starts_with("HTTP/1.1 206"), "{partial}");
        assert!(partial.contains("Content-Range: bytes 3-8/9"), "{partial}");
        assert!(partial.ends_with("hi</p>"), "{partial}");
        let token = probe(port).unwrap();
        assert_eq!(token, folder_token(&dir));
        assert!(!token.contains('/'), "the probe must not reveal the folder");
    }

    #[test]
    fn tailscale_status_gives_the_https_address() {
        let mock = crate::runner::MockRunner::default();
        mock.respond(
            "tailscale status",
            0,
            r#"{"BackendState":"Running","Self":{"DNSName":"create.taila09abe.ts.net."}}"#,
        );
        assert_eq!(
            tailscale_base(&mock, 8443).unwrap(),
            "https://create.taila09abe.ts.net:8443"
        );
        assert_eq!(
            tailscale_base(&mock, 443).unwrap(),
            "https://create.taila09abe.ts.net"
        );
        mock.respond(
            "tailscale status",
            0,
            r#"{"BackendState":"NeedsLogin","Self":{}}"#,
        );
        assert!(tailscale_base(&mock, 8443)
            .unwrap_err()
            .contains("tailscale up"));
    }

    const SERVE_STATUS: &str = r#"{
        "TCP": {"8443": {"HTTPS": true}, "9443": {"HTTPS": true}},
        "Web": {
            "create.t.ts.net:8443": {"Handlers": {"/": {"Proxy": "http://127.0.0.1:8411"}}},
            "create.t.ts.net:9443": {"Handlers": {"/": {"Proxy": "http://127.0.0.1:47411"}}}
        },
        "Foreground": {"abc": {"TCP": {"443": {"HTTPS": true}},
            "Web": {"create.t.ts.net:443": {"Handlers": {"/": {"Proxy": "http://127.0.0.1:80"}}}}}}
    }"#;

    #[test]
    fn reads_what_each_tailnet_port_serves() {
        let status: serde_json::Value = serde_json::from_str(SERVE_STATUS).unwrap();
        assert_eq!(
            serving_on(&status, 8443).as_deref(),
            Some("http://127.0.0.1:8411")
        );
        assert!(ours(&serving_on(&status, 9443).unwrap()));
        assert_eq!(
            serving_on(&status, 443).as_deref(),
            Some("http://127.0.0.1:80")
        );
        assert_eq!(serving_on(&status, 10443), None);
    }

    #[test]
    fn never_takes_over_a_port_that_serves_something_else() {
        let mock = crate::runner::MockRunner::default();
        mock.respond("tailscale serve status", 0, SERVE_STATUS);
        let err = tailscale_serve(&mock, 8443, LOCAL_PORT).unwrap_err();
        assert!(err.contains("8411") && err.contains("--port"), "{err}");
        // Already Forge's: nothing to change.
        tailscale_serve(&mock, 9443, LOCAL_PORT).unwrap();
        assert!(!mock.call_lines().iter().any(|l| l.contains("--bg")));
        // Free: serve it.
        mock.respond("timeout 60 tailscale serve --bg", 0, "");
        tailscale_serve(&mock, 10443, LOCAL_PORT).unwrap();
        assert!(mock
            .call_lines()
            .iter()
            .any(|l| l.contains("--https=10443 http://127.0.0.1:47411")));
    }

    #[test]
    fn stop_leaves_other_serves_alone() {
        let tmp = tempfile::tempdir().unwrap();
        let mock = crate::runner::MockRunner::default();
        mock.respond("tailscale serve status", 0, SERVE_STATUS);
        mock.respond("tailscale serve --https", 0, "");
        stop(&mock, tmp.path(), 8443).unwrap();
        assert!(!mock.call_lines().iter().any(|l| l.ends_with("off")));
        stop(&mock, tmp.path(), 9443).unwrap();
        assert!(mock
            .call_lines()
            .iter()
            .any(|l| l == "tailscale serve --https=9443 off"));
    }

    #[test]
    fn serve_errors_say_what_to_do() {
        assert!(explain_serve(
            "sending serve config: Access denied: serve config denied",
            Some(1),
            8443
        )
        .contains("--operator"));
        assert!(explain_serve(
            "updating config: foreground listener already exists for port 443",
            Some(1),
            443
        )
        .contains("--port"));
        assert!(explain_serve("", Some(124), 8443).contains("admin console"));
    }

    #[test]
    fn the_publish_folder_follows_xdg() {
        let env = |pairs: &'static [(&'static str, &'static str)]| {
            move |k: &str| {
                pairs
                    .iter()
                    .find(|(n, _)| *n == k)
                    .map(|(_, v)| v.to_string())
            }
        };
        assert_eq!(
            publish_dir(&env(&[("HOME", "/home/red")])),
            PathBuf::from("/home/red/.local/share/forge/publish")
        );
        assert_eq!(
            publish_dir(&env(&[("HOME", "/h"), ("XDG_DATA_HOME", "/d")])),
            PathBuf::from("/d/forge/publish")
        );
        assert_eq!(
            publish_dir(&env(&[("FORGE_PUBLISH_DIR", "/p")])),
            PathBuf::from("/p")
        );
    }
}
