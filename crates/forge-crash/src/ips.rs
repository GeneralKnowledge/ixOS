//! The `.ips` crash report format (iOS 15 and later): one line of JSON
//! metadata, then the report itself as one JSON document.
//!
//! Only the fields Forge uses are read. Everything is optional in practice
//! (reports differ between iOS versions and crash kinds), so a missing field
//! is an empty value, not an error.

use serde::Deserialize;
use serde_json::Value;

use crate::CrashError;

/// A parsed crash report, before symbolication.
#[derive(Debug, Clone, Default)]
pub struct Report {
    pub app: String,
    pub bundle_id: Option<String>,
    pub app_version: Option<String>,
    pub os: Option<String>,
    pub model: Option<String>,
    pub time: Option<String>,
    pub incident: Option<String>,
    pub exception: Exception,
    pub termination: Termination,
    /// What the crashing code said: Swift `Fatal error: ...`, an uncaught
    /// Objective-C exception's reason (the report's "application specific
    /// information").
    pub messages: Vec<String>,
    pub faulting_thread: usize,
    pub threads: Vec<Thread>,
    pub images: Vec<Image>,
    /// The stack where an Objective-C exception was thrown, when there was one.
    pub last_exception: Vec<RawFrame>,
    /// Index into `images` of the app's executable.
    pub main_image: Option<usize>,
}

#[derive(Debug, Clone, Default)]
pub struct Exception {
    pub kind: Option<String>,
    pub signal: Option<String>,
    pub codes: Option<String>,
    pub subtype: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct Termination {
    pub namespace: Option<String>,
    pub code: Option<u64>,
    pub indicator: Option<String>,
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct Thread {
    pub index: usize,
    pub name: Option<String>,
    pub queue: Option<String>,
    pub triggered: bool,
    pub frames: Vec<RawFrame>,
}

/// A frame as the phone wrote it: an image and an offset into it, plus the
/// phone's own symbol for it when the binary had a symbol table.
#[derive(Debug, Clone, Default)]
pub struct RawFrame {
    pub image: Option<usize>,
    pub offset: u64,
    pub symbol: Option<String>,
    pub symbol_offset: Option<u64>,
    pub file: Option<String>,
    pub line: Option<u32>,
}

#[derive(Debug, Clone, Default)]
pub struct Image {
    pub name: String,
    pub path: Option<String>,
    /// Lower-case, with dashes, as in the report.
    pub uuid: Option<String>,
    pub base: u64,
    pub size: u64,
    pub arch: Option<String>,
}

impl Image {
    /// Part of iOS rather than of the app: its frames can't be symbolicated
    /// from the app's build, and the phone already named what it could.
    pub fn is_system(&self) -> bool {
        let path = self.path.as_deref().unwrap_or("");
        path.starts_with("/usr/lib/")
            || path.starts_with("/System/")
            || path.starts_with("/Developer/")
            || path.starts_with("/private/preboot/Cryptexes/")
            || path.starts_with("/usr/libexec/")
    }
}

#[derive(Deserialize, Default)]
struct Header {
    app_name: Option<String>,
    name: Option<String>,
    #[serde(rename = "bundleID")]
    bundle_id: Option<String>,
    app_version: Option<String>,
    build_version: Option<String>,
    os_version: Option<String>,
    timestamp: Option<String>,
    incident_id: Option<String>,
    bug_type: Option<String>,
}

/// Parses an `.ips` file's bytes.
pub fn parse(bytes: &[u8]) -> Result<Report, CrashError> {
    let text = std::str::from_utf8(bytes).map_err(|_| CrashError::NotIps("not UTF-8 text"))?;
    let text = text.trim_start_matches('\u{feff}');
    if text.trim_start().starts_with("Incident Identifier:") {
        return Err(CrashError::NotIps(
            "this is an older text crash report (.crash); Forge reads .ips reports (iOS 15 and later)",
        ));
    }
    let (first, rest) = text.split_once('\n').unwrap_or((text, ""));
    let header: Header = serde_json::from_str(first.trim())
        .map_err(|_| CrashError::NotIps("the first line is not the .ips JSON header"))?;
    if let Some(bug) = header.bug_type.as_deref() {
        // 309 is a crash. Others (jetsam 298, hangs 409, CPU/disk resource
        // reports 145) have a different body.
        if bug != "309" && bug != "109" {
            return Err(CrashError::NotACrash(bug.to_string()));
        }
    }
    let body: Value = serde_json::from_str(rest.trim())
        .map_err(|e| CrashError::Malformed(format!("report body: {e}")))?;

    let s = |v: &Value, key: &str| v.get(key).and_then(Value::as_str).map(str::to_string);
    let mut report = Report {
        app: header
            .app_name
            .or(header.name)
            .or_else(|| s(&body, "procName"))
            .unwrap_or_else(|| "app".into()),
        bundle_id: header
            .bundle_id
            .or_else(|| s(&body["bundleInfo"], "CFBundleIdentifier")),
        app_version: match (header.app_version, header.build_version) {
            (Some(v), Some(b)) if !v.is_empty() => Some(format!("{v} ({b})")),
            (Some(v), _) if !v.is_empty() => Some(v),
            _ => None,
        },
        os: header.os_version,
        model: s(&body, "modelCode"),
        time: header.timestamp.or_else(|| s(&body, "captureTime")),
        incident: header.incident_id.or_else(|| s(&body, "incident")),
        ..Report::default()
    };

    let exc = &body["exception"];
    report.exception = Exception {
        kind: s(exc, "type"),
        signal: s(exc, "signal"),
        codes: s(exc, "codes"),
        subtype: s(exc, "subtype"),
    };
    let term = &body["termination"];
    report.termination = Termination {
        namespace: s(term, "namespace"),
        code: term.get("code").and_then(Value::as_u64),
        indicator: s(term, "indicator"),
        reasons: strings(&term["reasons"]),
    };

    // "asi": {"libswiftCore.dylib": ["Fatal error: ..."], ...}
    if let Some(asi) = body["asi"].as_object() {
        for lines in asi.values() {
            report.messages.extend(strings(lines));
        }
    }
    report.messages.retain(|m| !m.trim().is_empty());

    report.faulting_thread = body["faultingThread"].as_u64().unwrap_or(0) as usize;
    if let Some(threads) = body["threads"].as_array() {
        for (index, t) in threads.iter().enumerate() {
            report.threads.push(Thread {
                index,
                name: s(t, "name"),
                queue: s(t, "queue"),
                triggered: t["triggered"].as_bool().unwrap_or(false),
                frames: frames(&t["frames"]),
            });
        }
    }
    if !report.threads.iter().any(|t| t.triggered) {
        if let Some(t) = report.threads.get_mut(report.faulting_thread) {
            t.triggered = true;
        }
    } else if let Some(t) = report.threads.iter().find(|t| t.triggered) {
        report.faulting_thread = t.index;
    }
    report.last_exception = frames(&body["lastExceptionBacktrace"]);

    if let Some(images) = body["usedImages"].as_array() {
        for i in images {
            report.images.push(Image {
                name: s(i, "name").unwrap_or_default(),
                path: s(i, "path"),
                uuid: s(i, "uuid").map(|u| u.to_ascii_lowercase()),
                base: i["base"].as_u64().unwrap_or(0),
                size: i["size"].as_u64().unwrap_or(0),
                arch: s(i, "arch"),
            });
        }
    }
    let proc_path = s(&body, "procPath");
    let proc_name = s(&body, "procName").unwrap_or_else(|| report.app.clone());
    report.main_image = report
        .images
        .iter()
        .position(|i| proc_path.is_some() && i.path == proc_path)
        .or_else(|| report.images.iter().position(|i| i.name == proc_name));
    Ok(report)
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn frames(v: &Value) -> Vec<RawFrame> {
    let Some(list) = v.as_array() else {
        return Vec::new();
    };
    list.iter()
        .map(|f| RawFrame {
            image: f["imageIndex"].as_u64().map(|i| i as usize),
            offset: f["imageOffset"].as_u64().unwrap_or(0),
            symbol: f["symbol"].as_str().map(str::to_string),
            symbol_offset: f["symbolLocation"].as_u64(),
            file: f["sourceFile"].as_str().map(str::to_string),
            line: f["sourceLine"].as_u64().map(|l| l as u32),
        })
        .collect()
}
