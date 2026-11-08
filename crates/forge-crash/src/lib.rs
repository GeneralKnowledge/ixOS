//! Crash reports from an iPhone, read on Linux.
//!
//! [`analyze`] takes an `.ips` report (what `forge device crashes --pull`
//! copies off the phone) and gives back an [`Analysis`]: what kind of crash
//! it was, the message the app died with, the crashed thread's frames with
//! function, file and line, and the frame most likely at fault.
//!
//! Symbolication matches each image in the report to a local binary or dSYM
//! by its UUID, so only the exact build that crashed is used. The app's
//! binary from a debug build keeps a debug map that points at the object
//! files of the build, including the members of the app's static Rust
//! library, so Swift and Rust frames get their file and line from the same
//! lookup. Without a match, the report's own symbols (which the phone adds
//! from the symbol table) are used and the result says so.
//!
//! Nothing here needs a phone, Xcode or `atos`; Swift names are demangled by
//! the Swift toolchain when it is installed ([`demangle`]).

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use serde::Serialize;

pub mod demangle;
pub mod ips;
pub mod symbols;

pub use demangle::{NoSwift, SwiftDemangler, ToolchainSwift};

#[derive(Debug, thiserror::Error)]
pub enum CrashError {
    #[error("not an .ips crash report: {0}")]
    NotIps(&'static str),
    #[error(
        "this .ips report is not a crash (bug type {0}): Forge reads crash reports (bug type 309)"
    )]
    NotACrash(String),
    #[error("malformed .ips report: {0}")]
    Malformed(String),
}

/// Where to look for the binaries of the build that crashed.
#[derive(Debug, Clone, Default)]
pub struct Options {
    /// Folders searched for the app's binary and dSYMs (an app folder: its
    /// `.build` and `xtool` output).
    pub roots: Vec<PathBuf>,
    /// Binaries or `.dSYM` bundles given by hand; searched first.
    pub binaries: Vec<PathBuf>,
    /// Symbolicate every thread, not only the one that crashed.
    pub all_threads: bool,
}

/// What kind of crash this was.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// A Swift runtime check: force-unwrapped nil, `try!`, index out of
    /// range, overflow, `fatalError`, `precondition`.
    SwiftRuntimeError,
    /// A Rust panic that was not caught by a UniFFI call.
    RustPanic,
    /// An uncaught Objective-C exception (usually UIKit or Foundation misuse).
    ObjcException,
    /// A read or write of memory the app doesn't own.
    BadAccess,
    /// A trap instruction with no sign of where it came from.
    Trap,
    /// `abort()`.
    Abort,
    /// iOS killed the app because its main thread stopped responding.
    Watchdog,
    /// iOS refused the app's code signature.
    CodeSigning,
    Other,
}

impl Kind {
    pub fn title(self) -> &'static str {
        match self {
            Kind::SwiftRuntimeError => "Swift runtime error",
            Kind::RustPanic => "Rust panic",
            Kind::ObjcException => "uncaught Objective-C exception",
            Kind::BadAccess => "bad memory access",
            Kind::Trap => "trap",
            Kind::Abort => "abort",
            Kind::Watchdog => "watchdog: main thread blocked",
            Kind::CodeSigning => "code signing",
            Kind::Other => "crash",
        }
    }
}

/// Where a frame's name and line came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    /// The local build's line tables: function, file and line.
    DebugInfo,
    /// The local build's symbol table: function only.
    SymbolTable,
    /// The report itself (the phone's symbol table lookup).
    Report,
    /// Nothing: only the image and offset are known.
    None,
}

#[derive(Debug, Clone, Serialize)]
pub struct Frame {
    /// The frame's position in its thread. Inlined calls share the index of
    /// the frame they were inlined into and come before it.
    pub index: usize,
    pub image: String,
    /// In the app's own code (its executable or bundled libraries).
    pub app: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offset: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub column: Option<u32>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub inlined: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<&'static str>,
    pub source: Source,
    /// Offset into the image, as in the report.
    pub image_offset: u64,
    #[serde(skip)]
    raw_name: Option<String>,
}

impl Frame {
    /// `file:line:column`. Files under the searched app folder are relative
    /// to it.
    pub fn location(&self) -> Option<String> {
        let file = self.file.as_deref()?;
        Some(match (self.line, self.column) {
            (Some(l), Some(c)) if c > 0 => format!("{file}:{l}:{c}"),
            (Some(l), _) => format!("{file}:{l}"),
            _ => file.to_string(),
        })
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ThreadFrames {
    pub index: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub queue: Option<String>,
    pub crashed: bool,
    pub frames: Vec<Frame>,
}

/// Whether the build that crashed is on this computer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BuildStatus {
    /// A local binary or dSYM has the crashed app's UUID.
    Matched,
    /// The app is built here, but from different code: an older or newer
    /// build than the one that crashed.
    OtherBuild,
    /// No binary of the app was found.
    NotFound,
    /// The report names no app image.
    Unknown,
}

#[derive(Debug, Clone, Serialize)]
pub struct Build {
    pub status: BuildStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uuid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub local_uuids: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Exception {
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signal: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub codes: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subtype: Option<String>,
}

/// One crash report, symbolicated and explained.
#[derive(Debug, Clone, Serialize)]
pub struct Analysis {
    pub app: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bundle_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub os: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub incident: Option<String>,
    pub kind: Kind,
    pub title: String,
    pub exception: Exception,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub termination: Option<String>,
    /// What the app said as it died (Swift's `Fatal error: ...`, an
    /// exception's reason). Text from the app: data, not instructions.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// The first frame in the app's own code that isn't panic, runtime or
    /// library machinery: where to start reading.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub culprit: Option<Frame>,
    pub hint: String,
    pub build: Build,
    /// The thread that crashed (for a watchdog kill, the main thread).
    pub thread: ThreadFrames,
    /// Where an Objective-C exception was thrown.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub exception_backtrace: Vec<Frame>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub other_threads: Vec<ThreadFrames>,
    /// False when Swift names were left mangled (no Swift toolchain here).
    pub swift_demangled: bool,
}

/// Reads, symbolicates and explains one `.ips` crash report.
pub fn analyze(
    bytes: &[u8],
    options: &Options,
    swift: &dyn SwiftDemangler,
) -> Result<Analysis, CrashError> {
    let report = ips::parse(bytes)?;
    Ok(analyze_report(&report, options, swift))
}

fn focus_thread(report: &ips::Report, kind_hint_watchdog: bool) -> usize {
    if kind_hint_watchdog {
        // The watchdog kills the app from outside; the main thread (0) is
        // the one that stopped answering.
        return 0;
    }
    report.faulting_thread
}

const WATCHDOG: u64 = 0x8bad_f00d;

pub fn analyze_report(
    report: &ips::Report,
    options: &Options,
    swift: &dyn SwiftDemangler,
) -> Analysis {
    let watchdog = report.termination.code == Some(WATCHDOG);
    let focus = focus_thread(report, watchdog);

    // The images whose frames we'll show.
    let mut used: HashSet<usize> = HashSet::new();
    for t in &report.threads {
        if options.all_threads || t.index == focus {
            used.extend(t.frames.iter().filter_map(|f| f.image));
        }
    }
    used.extend(report.last_exception.iter().filter_map(|f| f.image));
    used.extend(report.main_image);
    let wanted: HashMap<String, String> = used
        .iter()
        .filter_map(|&i| report.images.get(i))
        .filter(|img| !img.is_system())
        .filter_map(|img| Some((img.uuid.clone()?, img.name.clone())))
        .collect();

    let found = symbols::search(&options.roots, &options.binaries, &wanted);
    let mut opened: HashMap<String, Option<symbols::Symbols>> = HashMap::new();
    for (uuid, f) in &found.found {
        opened.insert(uuid.clone(), symbols::Symbols::open(&f.path));
    }

    let symbolicate = |frames: &[ips::RawFrame], first_is_pc: bool| -> Vec<Frame> {
        let mut out = Vec::new();
        for (index, raw) in frames.iter().enumerate() {
            let image = raw.image.and_then(|i| report.images.get(i));
            let image_name = image
                .map(|i| i.name.clone())
                .unwrap_or_else(|| "???".into());
            let app = image.is_some_and(|i| !i.is_system());
            let syms = image
                .and_then(|i| i.uuid.as_ref())
                .and_then(|u| opened.get(u))
                .and_then(Option::as_ref);
            let return_address = !(first_is_pc && index == 0);
            let located = syms
                .map(|s| s.lookup(raw.offset, return_address))
                .unwrap_or_default();
            if located.is_empty() {
                let (source, name) = match &raw.symbol {
                    Some(s) => (Source::Report, Some(s.clone())),
                    None => (Source::None, None),
                };
                out.push(Frame {
                    index,
                    image: image_name,
                    app,
                    function: None,
                    offset: raw.symbol_offset.filter(|_| name.is_some()),
                    file: raw.file.clone(),
                    line: raw.line,
                    column: None,
                    inlined: false,
                    language: None,
                    source,
                    image_offset: raw.offset,
                    raw_name: name,
                });
                continue;
            }
            let outer_offset = syms.and_then(|s| s.symbol_offset(raw.offset));
            for l in located {
                let source = if l.file.is_some() {
                    Source::DebugInfo
                } else {
                    Source::SymbolTable
                };
                out.push(Frame {
                    index,
                    image: image_name.clone(),
                    app,
                    function: None,
                    offset: if l.inlined { None } else { outer_offset },
                    file: l.file,
                    line: l.line.filter(|&n| n > 0),
                    column: l.column,
                    inlined: l.inlined,
                    language: None,
                    source,
                    image_offset: raw.offset,
                    raw_name: l.function,
                });
            }
        }
        out
    };

    let mut thread_frames: Vec<ThreadFrames> = report
        .threads
        .iter()
        .filter(|t| options.all_threads || t.index == focus)
        .map(|t| ThreadFrames {
            index: t.index,
            name: t.name.clone(),
            queue: t.queue.clone(),
            crashed: t.triggered,
            frames: symbolicate(&t.frames, true),
        })
        .collect();
    let mut exception_backtrace = symbolicate(&report.last_exception, false);

    // Demangle every name in one go.
    let names: Vec<String> = thread_frames
        .iter()
        .flat_map(|t| t.frames.iter())
        .chain(exception_backtrace.iter())
        .filter_map(|f| f.raw_name.clone())
        .collect();
    let (readable, swift_demangled) = demangle::all(&names, swift);
    let roots: Vec<String> = options
        .roots
        .iter()
        .flat_map(|r| [r.clone(), r.canonicalize().unwrap_or_else(|_| r.clone())])
        .map(|r| r.display().to_string().trim_end_matches('/').to_string() + "/")
        .collect();
    let finish = |f: &mut Frame| {
        if let Some(file) = &f.file {
            if let Some(rel) = roots.iter().find_map(|r| file.strip_prefix(r.as_str())) {
                f.file = Some(rel.to_string());
            }
        }
        if let Some(raw) = &f.raw_name {
            f.function = Some(readable.get(raw).cloned().unwrap_or_else(|| raw.clone()));
            f.language = language(raw, f.file.as_deref());
        } else {
            f.language = language("", f.file.as_deref());
        }
    };
    for t in &mut thread_frames {
        t.frames.iter_mut().for_each(finish);
    }
    exception_backtrace.iter_mut().for_each(finish);

    let pos = thread_frames
        .iter()
        .position(|t| t.index == focus)
        .unwrap_or(0);
    let thread = if thread_frames.is_empty() {
        ThreadFrames {
            index: focus,
            name: None,
            queue: None,
            crashed: true,
            frames: Vec::new(),
        }
    } else {
        thread_frames.remove(pos)
    };

    let message = report
        .messages
        .iter()
        .chain(report.termination.reasons.iter())
        .find(|m| !m.trim().is_empty())
        .map(|m| m.trim().to_string());
    let kind = classify(report, &thread.frames, &exception_backtrace, watchdog);
    let culprit = culprit(&exception_backtrace)
        .or_else(|| culprit(&thread.frames))
        .cloned();

    let build = build_status(report, &found);
    let hint = hint(kind, report, message.as_deref(), culprit.as_ref(), &build);
    let termination = match (&report.termination.namespace, &report.termination.indicator) {
        (Some(ns), Some(ind)) => Some(format!("{ns}: {ind}")),
        (Some(ns), None) => Some(match report.termination.code {
            Some(c) => format!("{ns} code 0x{c:x}"),
            None => ns.clone(),
        }),
        (None, Some(ind)) => Some(ind.clone()),
        (None, None) => None,
    };

    Analysis {
        app: report.app.clone(),
        bundle_id: report.bundle_id.clone(),
        app_version: report.app_version.clone(),
        os: report.os.clone(),
        model: report.model.clone(),
        time: report.time.clone(),
        incident: report.incident.clone(),
        kind,
        title: kind.title().to_string(),
        exception: Exception {
            kind: report.exception.kind.clone(),
            signal: report.exception.signal.clone(),
            codes: report.exception.codes.clone(),
            subtype: report.exception.subtype.clone(),
        },
        termination,
        message,
        culprit,
        hint,
        build,
        thread,
        exception_backtrace,
        other_threads: thread_frames,
        swift_demangled,
    }
}

fn language(raw: &str, file: Option<&str>) -> Option<&'static str> {
    if demangle::is_swift_mangled(raw) {
        return Some("swift");
    }
    if demangle::rust(raw).is_some() {
        return Some("rust");
    }
    let ext = file?.rsplit('.').next()?;
    Some(match ext {
        "swift" => "swift",
        "rs" => "rust",
        "c" | "h" => "c",
        "m" | "mm" => "objc",
        "cpp" | "cc" | "hpp" => "c++",
        _ => return None,
    })
}

fn is_rust_panic_frame(f: &Frame) -> bool {
    let name = f.function.as_deref().unwrap_or("");
    name.starts_with("core::panicking")
        || name.starts_with("std::panicking")
        || name.starts_with("std::sys::backtrace::__rust_end_short_backtrace")
        || name.starts_with("rust_panic")
        || name.starts_with("__rust_start_panic")
        || name.starts_with("rust_begin_unwind")
        || name.starts_with("core::option::unwrap_failed")
        || name.starts_with("core::option::expect_failed")
        || name.starts_with("core::result::unwrap_failed")
}

fn is_swift_runtime_frame(f: &Frame) -> bool {
    let name = f.function.as_deref().unwrap_or("");
    name.contains("_assertionFailure")
        || name.contains("swift_runtimeFatal")
        || name.contains("fatalErrorMessage")
        || name.starts_with("Swift.fatalError")
        || name.starts_with("swift::fatalError")
        || name.contains("swift_unexpectedError")
}

/// Code the app didn't write: Rust's standard library and panic machinery,
/// the Swift runtime, UniFFI scaffolding, third-party crates.
fn is_machinery(f: &Frame) -> bool {
    let name = f.function.as_deref().unwrap_or("");
    let bare = name.trim_start_matches('<');
    let file = f.file.as_deref().unwrap_or("");
    [
        "core::",
        "std::",
        "alloc::",
        "rust_",
        "__rust",
        "Swift.",
        "swift_",
        "swift::",
        "_swift",
        "__swift",
        "uniffi_core::",
        "uniffi::",
        "abort",
        "__pthread",
        "_pthread",
    ]
    .iter()
    .any(|p| bare.starts_with(p))
        || is_swift_runtime_frame(f)
        || file.starts_with("/rustc/")
        || file.contains("/.cargo/registry/")
        || file.contains("/lib/rustlib/")
        || file.contains("/swift/stdlib/")
}

fn culprit(frames: &[Frame]) -> Option<&Frame> {
    frames
        .iter()
        .find(|f| f.app && f.function.is_some() && !is_machinery(f))
        .or_else(|| frames.iter().find(|f| f.app && f.function.is_some()))
}

fn classify(report: &ips::Report, frames: &[Frame], exc_bt: &[Frame], watchdog: bool) -> Kind {
    let exc = report.exception.kind.as_deref().unwrap_or("");
    let signal = report.exception.signal.as_deref().unwrap_or("");
    let said = |needle: &str| report.messages.iter().any(|m| m.contains(needle));
    if report.termination.namespace.as_deref() == Some("CODESIGNING") {
        return Kind::CodeSigning;
    }
    if watchdog {
        return Kind::Watchdog;
    }
    if frames.iter().any(is_rust_panic_frame) {
        return Kind::RustPanic;
    }
    if !exc_bt.is_empty()
        || frames
            .iter()
            .any(|f| f.function.as_deref() == Some("objc_exception_throw"))
        || said("uncaught exception")
    {
        return Kind::ObjcException;
    }
    let swift_said = said("Fatal error") || said("Precondition failed") || said("Assertion failed");
    let swift_frames = frames.iter().take(6).any(is_swift_runtime_frame);
    if swift_said || swift_frames {
        return Kind::SwiftRuntimeError;
    }
    match exc {
        "EXC_BREAKPOINT" | "EXC_BAD_INSTRUCTION" => {
            // Swift traps in place (brk #1) for most runtime checks, with no
            // runtime frame on the stack: a Swift frame on top says so.
            let top_app = frames.iter().find(|f| f.app);
            if top_app.is_some_and(|f| f.language == Some("swift")) {
                Kind::SwiftRuntimeError
            } else {
                Kind::Trap
            }
        }
        "EXC_BAD_ACCESS" => Kind::BadAccess,
        _ if signal == "SIGABRT" => Kind::Abort,
        _ if signal == "SIGSEGV" || signal == "SIGBUS" => Kind::BadAccess,
        _ => Kind::Other,
    }
}

fn build_status(report: &ips::Report, found: &symbols::Search) -> Build {
    let Some(main) = report.main_image.and_then(|i| report.images.get(i)) else {
        return Build {
            status: BuildStatus::Unknown,
            uuid: None,
            path: None,
            local_uuids: Vec::new(),
        };
    };
    let uuid = main.uuid.clone();
    if let Some(f) = uuid.as_ref().and_then(|u| found.found.get(u)) {
        return Build {
            status: BuildStatus::Matched,
            uuid,
            path: Some(f.path.display().to_string()),
            local_uuids: Vec::new(),
        };
    }
    let mut others: Vec<String> = found
        .other_builds
        .get(&main.name)
        .map(|v| v.iter().map(|(u, _)| u.clone()).collect())
        .unwrap_or_default();
    others.sort();
    others.dedup();
    Build {
        status: if others.is_empty() {
            BuildStatus::NotFound
        } else {
            BuildStatus::OtherBuild
        },
        uuid,
        path: None,
        local_uuids: others,
    }
}

fn at(f: Option<&Frame>) -> String {
    match f {
        Some(f) => {
            let name = f.function.as_deref().unwrap_or("?");
            match f.location() {
                Some(loc) => format!(" in {name} ({loc})"),
                None => format!(" in {name}"),
            }
        }
        None => String::new(),
    }
}

fn hint(
    kind: Kind,
    report: &ips::Report,
    message: Option<&str>,
    culprit: Option<&Frame>,
    build: &Build,
) -> String {
    let at = at(culprit);
    let mut hint = match kind {
        Kind::SwiftRuntimeError => format!(
            "A Swift runtime check stopped the app{at}. Look for a force unwrap (!), try!, an array index out of range, integer overflow, or fatalError/precondition on that line."
        ),
        Kind::RustPanic => format!(
            "Rust panicked{at} and the panic was not caught by a UniFFI call (a background thread, a callback, or a panic while unwinding), so the app aborted. forge_runtime logs the panic message as `[panic]` in the system log: keep `forge device logs --grep panic` running while you reproduce. Return an error across the boundary instead of panicking."
        ),
        Kind::ObjcException => format!(
            "An Objective-C exception was not caught{at}. This is usually a UIKit or Foundation API used wrongly; the message says which."
        ),
        Kind::BadAccess => {
            let null = report
                .exception
                .subtype
                .as_deref()
                .is_some_and(|s| s.ends_with(" 0x0000000000000000") || s.ends_with(" 0x0"));
            format!(
                "The app touched memory it doesn't own{at}{}. In Swift look at unowned references, unsafe pointers and data races; in Rust at unsafe code and FFI handles used after they were freed.",
                if null { " (a null pointer)" } else { "" }
            )
        }
        Kind::Trap => format!(
            "The app hit a trap instruction{at}: a Swift runtime check (force unwrap, overflow, index out of range) or a Rust abort."
        ),
        Kind::Abort => format!(
            "The app called abort(){at}. Check the message and the frames above it."
        ),
        Kind::Watchdog => format!(
            "iOS killed the app because the main thread was blocked too long{at}. The main thread's frames show what it was waiting on: move network, disk or long Rust calls off the main thread."
        ),
        Kind::CodeSigning => "iOS refused the app's signature. Apps signed with a free Apple team expire after 7 days: reinstall with `forge run ios`.".to_string(),
        Kind::Other => format!("The app crashed{at}."),
    };
    if let Some(m) = message {
        if kind != Kind::CodeSigning && !m.is_empty() {
            hint = format!(
                "{hint} The app said: \"{}\".",
                m.lines().next().unwrap_or(m)
            );
        }
    }
    match build.status {
        BuildStatus::OtherBuild => hint.push_str(
            " The local build is not the one that crashed, so there are no file and line numbers: rebuild and reproduce the crash to get them.",
        ),
        BuildStatus::NotFound => hint.push_str(
            " No local binary of the app was found: run this from the app folder after a build, or pass --binary.",
        ),
        _ => {}
    }
    hint
}

/// The analysis as text for a terminal.
pub fn render(a: &Analysis, max_frames: usize) -> String {
    use std::fmt::Write;
    let mut s = String::new();
    let exc = match (&a.exception.kind, &a.exception.signal) {
        (Some(k), Some(sig)) => format!(" ({k}, {sig})"),
        (Some(k), None) => format!(" ({k})"),
        (None, Some(sig)) => format!(" ({sig})"),
        _ => String::new(),
    };
    let _ = writeln!(s, "{} crashed: {}{exc}", a.app, a.title);
    if let Some(m) = &a.message {
        for line in m.lines().take(4) {
            let _ = writeln!(s, "  {line}");
        }
    }
    if let Some(sub) = &a.exception.subtype {
        let _ = writeln!(s, "  {sub}");
    }
    if let Some(t) = &a.termination {
        let _ = writeln!(s, "  Termination: {t}");
    }
    let mut when = Vec::new();
    when.extend(a.time.clone());
    when.extend(a.os.clone());
    when.extend(a.model.clone());
    when.extend(a.app_version.as_ref().map(|v| format!("app {v}")));
    if !when.is_empty() {
        let _ = writeln!(s, "  {}", when.join(" · "));
    }
    let build = match a.build.status {
        BuildStatus::Matched => format!(
            "matches {}",
            a.build.path.as_deref().unwrap_or("a local build")
        ),
        BuildStatus::OtherBuild => format!(
            "not on this computer (crashed build {}, local {})",
            a.build.uuid.as_deref().unwrap_or("?"),
            a.build.local_uuids.join(", ")
        ),
        BuildStatus::NotFound => "no local binary found".into(),
        BuildStatus::Unknown => "unknown".into(),
    };
    let _ = writeln!(s, "  Build: {build}");
    if let Some(c) = &a.culprit {
        let loc = c.location().map(|l| format!(" at {l}")).unwrap_or_default();
        let _ = writeln!(
            s,
            "\nLikely cause: {}{loc}",
            c.function.as_deref().unwrap_or("?")
        );
    }
    let _ = writeln!(s, "Hint: {}", a.hint);
    if !a.swift_demangled {
        let _ = writeln!(
            s,
            "(Swift names are mangled: install the Swift toolchain, or run `swift demangle` on them.)"
        );
    }
    if !a.exception_backtrace.is_empty() {
        let _ = writeln!(s, "\nException thrown at:");
        render_frames(&mut s, &a.exception_backtrace, max_frames);
    }
    let mut threads = vec![&a.thread];
    threads.extend(a.other_threads.iter());
    for t in threads {
        let mut label = format!("Thread {}", t.index);
        let mut extra = Vec::new();
        if t.crashed {
            extra.push("crashed".to_string());
        }
        extra.extend(t.name.clone());
        extra.extend(t.queue.clone());
        if !extra.is_empty() {
            label = format!("{label} ({})", extra.join(", "));
        }
        let _ = writeln!(s, "\n{label}:");
        render_frames(&mut s, &t.frames, max_frames);
    }
    s
}

fn render_frames(s: &mut String, frames: &[Frame], max_frames: usize) {
    use std::fmt::Write;
    let width = frames
        .iter()
        .take(max_frames)
        .map(|f| f.image.len())
        .max()
        .unwrap_or(0)
        .min(28);
    for (shown, f) in frames.iter().enumerate() {
        if shown >= max_frames {
            let rest = frames.len() - shown;
            let _ = writeln!(s, "  ... {rest} more");
            break;
        }
        let name = match (&f.function, f.offset) {
            (Some(n), Some(o)) if f.file.is_none() => format!("{n} + {o}"),
            (Some(n), _) => n.clone(),
            (None, _) => format!("0x{:x}", f.image_offset),
        };
        let loc = f.location().map(|l| format!("  {l}")).unwrap_or_default();
        let index = if f.inlined {
            "  ".to_string()
        } else {
            format!("{:>2}", f.index)
        };
        let inl = if f.inlined { " [inlined]" } else { "" };
        let _ = writeln!(s, "  {index} {:<width$}  {name}{inl}{loc}", f.image);
    }
}
