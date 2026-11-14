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

/// One published app, as `apps.json` lists it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
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

pub fn manifest_url(base: &str, bundle_id: &str) -> String {
    format!("{}/{bundle_id}/manifest.plist", base.trim_end_matches('/'))
}

/// The link iOS follows to install (Safari asks first).
pub fn install_url(base: &str, bundle_id: &str) -> String {
    format!(
        "itms-services://?action=download-manifest&url={}",
        manifest_url(base, bundle_id)
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
            "{}/{}/app.ipa?v={}",
            base.trim_end_matches('/'),
            entry.bundle_id,
            entry.published
        )
        .into(),
    );
    let mut metadata = Dictionary::new();
    metadata.insert("bundle-identifier".into(), entry.bundle_id.clone().into());
    metadata.insert("bundle-version".into(), entry.version.clone().into());
    metadata.insert("kind".into(), "software".into());
    metadata.insert("title".into(), entry.name.clone().into());
    let mut item = Dictionary::new();
    item.insert(
        "assets".into(),
        Value::Array(vec![Value::Dictionary(asset)]),
    );
    item.insert("metadata".into(), Value::Dictionary(metadata));
    let mut root = Dictionary::new();
    root.insert("items".into(), Value::Array(vec![Value::Dictionary(item)]));
    let mut out = Vec::new();
    Value::Dictionary(root)
        .to_writer_xml(&mut out)
        .map_err(|e| format!("cannot write the manifest: {e}"))?;
    Ok(out)
}

fn read_entries(dir: &Path) -> Vec<Entry> {
    fs::read(dir.join(APPS_JSON))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

/// Writes `bytes` to `path` through a temporary file, so the server never
/// hands out half a file.
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let tmp = path.with_extension("partial");
    fs::write(&tmp, bytes).map_err(|e| format!("cannot write {}: {e}", tmp.display()))?;
    fs::rename(&tmp, path).map_err(|e| format!("cannot write {}: {e}", path.display()))
}

/// Copies the `.ipa` into the folder and rewrites every manifest, `apps.json`
/// and the page for `base` (the HTTPS address the phone uses).
pub fn stage(
    dir: &Path,
    ipa: &Path,
    info: &IpaInfo,
    base: &str,
    commit: Option<String>,
    now: u64,
) -> Result<Entry, String> {
    safe_id(&info.bundle_id)?;
    let app_dir = dir.join(&info.bundle_id);
    fs::create_dir_all(&app_dir)
        .map_err(|e| format!("cannot create {}: {e}", app_dir.display()))?;
    let tmp = app_dir.join("app.ipa.partial");
    let size = fs::copy(ipa, &tmp).map_err(|e| format!("cannot copy {}: {e}", ipa.display()))?;
    fs::rename(&tmp, app_dir.join("app.ipa"))
        .map_err(|e| format!("cannot publish the .ipa: {e}"))?;
    let entry = Entry {
        name: info.name.clone(),
        bundle_id: info.bundle_id.clone(),
        version: info.version.clone(),
        build: info.build.clone(),
        published: now,
        expires: info.expires,
        commit,
        size,
    };
    let mut entries = read_entries(dir);
    entries.retain(|e| e.bundle_id != entry.bundle_id);
    entries.push(entry.clone());
    entries.sort_by_key(|e| std::cmp::Reverse(e.published));
    rewrite(dir, &entries, base)?;
    Ok(entry)
}

fn rewrite(dir: &Path, entries: &[Entry], base: &str) -> Result<(), String> {
    for e in entries {
        if safe_id(&e.bundle_id).is_ok() && dir.join(&e.bundle_id).is_dir() {
            write_atomic(
                &dir.join(&e.bundle_id).join("manifest.plist"),
                &manifest(base, e)?,
            )?;
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

/// The page the phone opens: every published app, newest first.
pub fn index_html(base: &str, entries: &[Entry]) -> String {
    let mut cards = String::new();
    for e in entries {
        let mut details = format!("Published {}", when(e.published));
        if let Some(c) = &e.commit {
            details.push_str(&format!(" · {}", esc(c)));
        }
        let expiry = e
            .expires
            .map(|x| {
                format!(
                    r#"<p class="expires" data-expires="{x}">Signed until {}</p>"#,
                    when(x)
                )
            })
            .unwrap_or_default();
        cards.push_str(&format!(
            r#"<section>
<h2>{name}</h2>
<p class="version">{version} ({build})</p>
<p>{details}</p>
{expiry}<a class="install" href="{href}" data-testid="install.{id}">Install {name}</a>
</section>
"#,
            name = esc(&e.name),
            version = esc(&e.version),
            build = esc(&e.build),
            href = esc(&install_url(base, &e.bundle_id)),
            id = esc(&e.bundle_id),
        ));
    }
    if cards.is_empty() {
        cards.push_str(
            "<p>Nothing published yet. On the laptop: <code>forge publish ios</code></p>",
        );
    }
    format!(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Forge builds</title>
<style>
:root {{ color-scheme: light dark; --bg: #f2f2f7; --card: #fff; --text: #1c1c1e; --muted: #6c6c70; --accent: #007aff; --warn: #c93400; }}
@media (prefers-color-scheme: dark) {{ :root {{ --bg: #000; --card: #1c1c1e; --text: #f2f2f7; --muted: #98989f; --accent: #0a84ff; --warn: #ff9f0a; }} }}
body {{ margin: 0; padding: 16px; background: var(--bg); color: var(--text); font: 17px/1.4 -apple-system, system-ui, sans-serif; }}
h1 {{ font-size: 28px; margin: 8px 0 4px; }}
.hint {{ color: var(--muted); margin: 0 0 16px; }}
section {{ background: var(--card); border-radius: 12px; padding: 16px; margin: 0 0 12px; }}
h2 {{ margin: 0; font-size: 20px; }}
p {{ margin: 4px 0; }}
.version, section p {{ color: var(--muted); }}
.expired {{ color: var(--warn); font-weight: 600; }}
.install {{ display: block; margin-top: 12px; padding: 12px; border-radius: 10px; background: var(--accent); color: #fff; text-align: center; font-weight: 600; text-decoration: none; }}
</style>
</head>
<body>
<h1>Forge builds</h1>
<p class="hint">Tap Install, then Install again. The app updates in place and keeps its data.</p>
{cards}<script>
for (const p of document.querySelectorAll("[data-expires]")) {{
  if (Number(p.dataset.expires) * 1000 < Date.now()) {{ p.textContent = "Signature expired: publish it again"; p.className = "expired"; }}
}}
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
        [id, file @ ("manifest.plist" | "app.ipa")] if safe_id(id).is_ok() => {
            Some(dir.join(id).join(file))
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
    // Skip the headers.
    let mut header = String::new();
    while reader.read_line(&mut header).is_ok_and(|n| n > 2) {
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
    let _ = write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: {}\r\nContent-Length: {len}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        content_type(&path)
    );
    if !head {
        let _ = std::io::copy(&mut file, &mut stream);
    }
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
    commit: Option<String>,
) -> Result<Published, String> {
    let info = read_ipa(ipa)?;
    let custom = base_url.is_some();
    let base = match base_url {
        Some(url) => url.trim_end_matches('/').to_string(),
        None => tailscale_base(runner, https_port)?,
    };
    fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let entry = stage(dir, ipa, &info, &base, commit, now)?;
    ensure_server(dir, LOCAL_PORT)?;
    // With --url, whatever serves that address is the person's own setup.
    if !custom {
        tailscale_serve(runner, https_port, LOCAL_PORT)?;
    }
    Ok(Published {
        page: page_url(&base),
        install: install_url(&base, &entry.bundle_id),
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
    fn the_manifest_names_the_signed_bundle_id_and_the_ipa() {
        let entry = Entry {
            name: "Device Lab".into(),
            bundle_id: "XTL-T.org.x".into(),
            version: "0.3".into(),
            build: "7".into(),
            published: 100,
            expires: None,
            commit: None,
            size: 1,
        };
        let xml =
            String::from_utf8(manifest("https://create.t.ts.net:8443/", &entry).unwrap()).unwrap();
        assert!(
            xml.contains("<string>https://create.t.ts.net:8443/XTL-T.org.x/app.ipa?v=100</string>"),
            "{xml}"
        );
        assert!(
            xml.contains("<key>bundle-identifier</key>\n\t\t\t\t<string>XTL-T.org.x</string>")
                || xml.contains("<string>XTL-T.org.x</string>")
        );
        assert!(xml.contains("<string>software-package</string>"));
        assert_eq!(
            install_url("https://h.ts.net:8443", "XTL-T.org.x"),
            "itms-services://?action=download-manifest&url=https://h.ts.net:8443/XTL-T.org.x/manifest.plist"
        );
    }

    #[test]
    fn staging_keeps_one_entry_per_app_newest_first() {
        let tmp = tempfile::tempdir().unwrap();
        let ipa = tmp.path().join("App.ipa");
        write_ipa(&ipa);
        let dir = tmp.path().join("pub");
        let mut info = read_ipa(&ipa).unwrap();
        stage(&dir, &ipa, &info, "https://h.ts.net:8443", None, 10).unwrap();
        info.bundle_id = "XTL-T.org.other".into();
        info.name = "Other".into();
        stage(
            &dir,
            &ipa,
            &info,
            "https://h.ts.net:8443",
            Some("abc1234".into()),
            20,
        )
        .unwrap();
        info.bundle_id = "XTL-ND4P87PX52.org.forgemobile.devicelab".into();
        info.name = "Device Lab".into();
        stage(&dir, &ipa, &info, "https://h.ts.net:8443", None, 30).unwrap();

        let entries = read_entries(&dir);
        let order: Vec<_> = entries
            .iter()
            .map(|e| (e.name.as_str(), e.published))
            .collect();
        assert_eq!(order, [("Device Lab", 30), ("Other", 20)]);
        assert!(dir.join("XTL-T.org.other/app.ipa").is_file());
        assert!(dir.join("XTL-T.org.other/manifest.plist").is_file());
        let page = fs::read_to_string(dir.join("index.html")).unwrap();
        assert!(page.find("Device Lab").unwrap() < page.find("Other").unwrap());
        assert!(page.contains("abc1234"));
        assert!(page.contains("itms-services://?action=download-manifest&amp;url=https://h.ts.net:8443/XTL-T.org.other/manifest.plist"));
        assert!(page.contains("Signed until 2026-11-21 13:24 UTC"));
    }

    #[test]
    fn unsafe_bundle_ids_are_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let ipa = tmp.path().join("App.ipa");
        write_ipa(&ipa);
        let mut info = read_ipa(&ipa).unwrap();
        info.bundle_id = "../escape".into();
        assert!(stage(tmp.path(), &ipa, &info, "https://h", None, 1).is_err());
    }

    #[test]
    fn the_server_never_leaves_its_folder() {
        let dir = Path::new("/srv/pub");
        assert_eq!(resolve(dir, "/a/app.ipa?v=3"), Some(dir.join("a/app.ipa")));
        assert_eq!(
            resolve(dir, "/a/manifest.plist"),
            Some(dir.join("a/manifest.plist"))
        );
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
        fs::create_dir(dir.join("a")).unwrap();
        fs::write(dir.join("a/manifest.plist"), "<plist/>").unwrap();
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
        let manifest = get("/a/manifest.plist");
        assert!(manifest.contains("Content-Type: text/xml"), "{manifest}");
        assert!(get("/missing").starts_with("HTTP/1.1 404"));
        assert!(get("/../x").starts_with("HTTP/1.1 404"));
        assert!(get("/server.pid").starts_with("HTTP/1.1 404"));
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
