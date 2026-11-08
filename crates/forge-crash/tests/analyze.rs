//! End-to-end: real Mach-O debug info, real report layouts.

use std::path::{Path, PathBuf};

use forge_crash::{analyze, BuildStatus, Kind, NoSwift, Options, Source};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn read(name: &str) -> Vec<u8> {
    std::fs::read(fixtures().join(name)).unwrap()
}

#[test]
fn symbolicates_against_a_dsym_found_by_uuid() {
    // The dSYM sits in an app-like folder; the search finds it by UUID.
    let app = tempfile::tempdir().unwrap();
    let build = app.path().join(".build/arm64-apple-ios/debug");
    copy_dir(&fixtures().join("Crashy.dSYM"), &build.join("Crashy.dSYM"));
    let options = Options {
        roots: vec![app.path().to_path_buf()],
        ..Options::default()
    };
    let a = analyze(&read("Crashy-2026-11-08-101500.ips"), &options, &NoSwift).unwrap();

    assert_eq!(a.build.status, BuildStatus::Matched, "{:?}", a.build);
    assert_eq!(a.kind, Kind::Trap);
    let top: Vec<_> = a
        .thread
        .frames
        .iter()
        .map(|f| (f.function.clone().unwrap_or_default(), f.line, f.source))
        .collect();
    // brk in trap_here (line 5); the calls in depth (11) and main (15).
    assert_eq!(top[0], ("trap_here".into(), Some(5), Source::DebugInfo));
    assert_eq!(top[1], ("depth".into(), Some(11), Source::DebugInfo));
    assert_eq!(top[2], ("main".into(), Some(15), Source::DebugInfo));
    // dyld is iOS's: the report's own symbol.
    assert_eq!(top[3], ("start".into(), None, Source::Report));
    assert!(!a.thread.frames[3].app);

    let culprit = a.culprit.as_ref().unwrap();
    assert_eq!(culprit.function.as_deref(), Some("trap_here"));
    assert!(culprit.file.as_deref().unwrap().ends_with("crashy.c"));
    assert_eq!(a.thread.queue.as_deref(), Some("com.apple.main-thread"));
    assert!(a.other_threads.is_empty());

    let text = forge_crash::render(&a, 30);
    assert!(
        text.starts_with("Crashy crashed: trap (EXC_BREAKPOINT, SIGTRAP)\n"),
        "{text}"
    );
    assert!(text.contains("Likely cause: trap_here at "), "{text}");
    assert!(text.contains("crashy.c:5:9"), "{text}");
    assert!(
        text.contains("Thread 0 (crashed, com.apple.main-thread):"),
        "{text}"
    );
}

#[test]
fn an_explicit_binary_and_all_threads() {
    let options = Options {
        binaries: vec![fixtures().join("Crashy.dSYM")],
        all_threads: true,
        ..Options::default()
    };
    let a = analyze(&read("Crashy-2026-11-08-101500.ips"), &options, &NoSwift).unwrap();
    assert_eq!(a.build.status, BuildStatus::Matched);
    assert_eq!(a.other_threads.len(), 1);
    assert_eq!(
        a.other_threads[0].frames[0].function.as_deref(),
        Some("__workq_kernreturn")
    );
}

#[test]
fn a_different_local_build_is_reported_not_used() {
    // A binary with the app's name but another UUID: an older or newer build.
    let app = tempfile::tempdir().unwrap();
    let dir = app.path().join("xtool/Crashy.app");
    std::fs::create_dir_all(&dir).unwrap();
    let mut other =
        std::fs::read(fixtures().join("Crashy.dSYM/Contents/Resources/DWARF/Crashy")).unwrap();
    let uuid = [0x4c, 0x4c, 0x44, 0x51, 0x55, 0x55, 0x31, 0x44];
    let at = other.windows(8).position(|w| w == uuid).unwrap();
    other[at] = 0x5d;
    std::fs::write(dir.join("Crashy"), other).unwrap();

    let options = Options {
        roots: vec![app.path().to_path_buf()],
        ..Options::default()
    };
    let a = analyze(&read("Crashy-2026-11-08-101500.ips"), &options, &NoSwift).unwrap();
    assert_eq!(a.build.status, BuildStatus::OtherBuild);
    assert_eq!(
        a.build.local_uuids,
        vec!["5d4c4451-5555-3144-a1b5-b63c7a520c29"]
    );
    // Falls back to the phone's symbols, without lines.
    assert_eq!(a.thread.frames[0].function.as_deref(), Some("trap_here"));
    assert_eq!(a.thread.frames[0].source, Source::Report);
    assert!(a.hint.contains("rebuild and reproduce"), "{}", a.hint);
}

#[test]
fn a_rust_panic_points_past_the_panic_machinery() {
    let a = analyze(
        &read("ChatXtool-2026-11-08-093000.ips"),
        &Options::default(),
        &NoSwift,
    )
    .unwrap();
    assert_eq!(a.kind, Kind::RustPanic);
    assert_eq!(a.thread.index, 3);
    assert_eq!(a.thread.name.as_deref(), Some("tokio-runtime-worker"));
    let c = a.culprit.unwrap();
    assert_eq!(
        c.function.as_deref(),
        Some("chat_core::room::Room::apply_remote")
    );
    assert_eq!(c.language, Some("rust"));
    assert_eq!(c.offset, Some(412));
    assert_eq!(a.build.status, BuildStatus::NotFound);
    assert!(
        a.hint.contains("forge device logs --grep panic"),
        "{}",
        a.hint
    );
    assert_eq!(a.app_version.as_deref(), Some("1.0 (7)"));
}

/// The debug map path: no dSYM, the binary points at its object files, as
/// in an xtool debug build. Needs clang and ld64.lld, so it only runs where
/// they are installed.
#[test]
fn symbolicates_through_the_debug_map_when_llvm_is_here() {
    let lld = ["ld64.lld", "ld64.lld-18", "ld64.lld-19", "ld64.lld-20"]
        .into_iter()
        .find(|t| {
            std::process::Command::new(t)
                .arg("--version")
                .output()
                .is_ok()
        });
    let (Some(lld), true) = (
        lld,
        std::process::Command::new("clang")
            .arg("--version")
            .output()
            .is_ok(),
    ) else {
        eprintln!("skipped: clang and ld64.lld are not installed");
        return;
    };
    let app = tempfile::tempdir().unwrap();
    let dir = app.path().join(".build/arm64-apple-ios/debug");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::copy(fixtures().join("crashy.c"), dir.join("crashy.c")).unwrap();
    let ok = std::process::Command::new("clang")
        .args([
            "-target",
            "arm64-apple-ios17.0",
            "-g",
            "-O0",
            "-c",
            "crashy.c",
            "-o",
            "crashy.o",
        ])
        .current_dir(&dir)
        .status()
        .unwrap()
        .success()
        && std::process::Command::new(lld)
            .args([
                "-arch",
                "arm64",
                "-platform_version",
                "ios",
                "17.0",
                "17.0",
                "-e",
                "_main",
                "-o",
                "Crashy",
                "crashy.o",
            ])
            .current_dir(&dir)
            .status()
            .unwrap()
            .success();
    assert!(ok, "building the fixture failed");
    let uuid = forge_crash::symbols::file_uuid(&dir.join("Crashy")).unwrap();
    let report = String::from_utf8(read("Crashy-2026-11-08-101500.ips"))
        .unwrap()
        .replace("4c4c4451-5555-3144-a1b5-b63c7a520c29", &uuid);
    let options = Options {
        roots: vec![app.path().to_path_buf()],
        ..Options::default()
    };
    let a = analyze(report.as_bytes(), &options, &NoSwift).unwrap();
    assert_eq!(a.build.status, BuildStatus::Matched);
    let f = &a.thread.frames[0];
    assert_eq!(f.function.as_deref(), Some("trap_here"));
    assert_eq!(f.line, Some(5));
    // Under the app folder: shown relative to it.
    assert_eq!(
        f.file.as_deref(),
        Some(".build/arm64-apple-ios/debug/crashy.c")
    );
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        let dest = to.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            copy_dir(&e.path(), &dest);
        } else {
            std::fs::copy(e.path(), dest).unwrap();
        }
    }
}

#[test]
fn a_build_copy_with_another_name_is_found_by_uuid() {
    let app = tempfile::tempdir().unwrap();
    let dir = app.path().join(".build/arm64-apple-ios/debug");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::copy(
        fixtures().join("Crashy.dSYM/Contents/Resources/DWARF/Crashy"),
        dir.join("Crashy-linked"),
    )
    .unwrap();
    let options = Options {
        roots: vec![app.path().to_path_buf()],
        ..Options::default()
    };
    let a = analyze(&read("Crashy-2026-11-08-101500.ips"), &options, &NoSwift).unwrap();
    assert_eq!(a.build.status, BuildStatus::Matched);
    assert_eq!(a.thread.frames[0].line, Some(5));
}

/// A report around `body`, with one app frame named by the phone.
fn report(exception: &str, termination: &str, extra: &str, symbol: &str) -> Vec<u8> {
    format!(
        r#"{{"app_name":"MyAppXtool","bug_type":"309","os_version":"iPhone OS 26.3.1 (23D8133)"}}
{{"procName":"MyAppXtool","exception":{exception},"termination":{termination},{extra}
"faultingThread":0,"threads":[{{"triggered":true,"queue":"com.apple.main-thread","frames":[
{{"imageOffset":100,"symbol":"{symbol}","symbolLocation":20,"imageIndex":0}}]}}],
"usedImages":[{{"base":4294967296,"size":4096,"uuid":"11111111-2222-3333-4444-555555555555","path":"/private/var/containers/Bundle/Application/X/MyAppXtool.app/MyAppXtool","name":"MyAppXtool"}}]}}"#
    )
    .into_bytes()
}

#[test]
fn kinds_of_crash() {
    let kind = |bytes: Vec<u8>| analyze(&bytes, &Options::default(), &NoSwift).unwrap();

    let swift = kind(report(
        r#"{"type":"EXC_BREAKPOINT","signal":"SIGTRAP"}"#,
        r#"{"namespace":"SIGNAL","code":5}"#,
        r#""asi":{"libswiftCore.dylib":["MyApp/ListView.swift:42: Fatal error: Unexpectedly found nil while unwrapping an Optional value"]},"#,
        "$s9MyAppXtool8ListViewV4bodyQrvg",
    ));
    assert_eq!(swift.kind, Kind::SwiftRuntimeError);
    assert!(swift
        .message
        .as_deref()
        .unwrap()
        .contains("Unexpectedly found nil"));
    assert_eq!(swift.culprit.as_ref().unwrap().language, Some("swift"));
    assert!(!swift.swift_demangled);

    // A Swift trap with no message: the Swift frame on top says it.
    let trap = kind(report(
        r#"{"type":"EXC_BREAKPOINT","signal":"SIGTRAP"}"#,
        r#"{"namespace":"SIGNAL","code":5}"#,
        "",
        "$s9MyAppXtool5ModelC3addyyF",
    ));
    assert_eq!(trap.kind, Kind::SwiftRuntimeError);

    let watchdog = kind(report(
        r#"{"type":"EXC_CRASH","signal":"SIGKILL"}"#,
        r#"{"namespace":"FRONTBOARD","code":2343432205,"reasons":["scene-update watchdog transgression"]}"#,
        "",
        "$s9MyAppXtool5ModelC4loadyyF",
    ));
    assert_eq!(watchdog.kind, Kind::Watchdog);
    assert!(watchdog.hint.contains("main thread"), "{}", watchdog.hint);

    let objc = kind(report(
        r#"{"type":"EXC_CRASH","signal":"SIGABRT"}"#,
        r#"{"namespace":"SIGNAL","code":6}"#,
        r#""asi":{"CoreFoundation":["*** Terminating app due to uncaught exception 'NSInvalidArgumentException'"]},"#,
        "main",
    ));
    assert_eq!(objc.kind, Kind::ObjcException);

    let null = kind(report(
        r#"{"type":"EXC_BAD_ACCESS","signal":"SIGSEGV","subtype":"KERN_INVALID_ADDRESS at 0x0000000000000000"}"#,
        r#"{"namespace":"SIGNAL","code":11}"#,
        "",
        "main",
    ));
    assert_eq!(null.kind, Kind::BadAccess);
    assert!(null.hint.contains("null pointer"), "{}", null.hint);

    let signing = kind(report(
        r#"{"type":"EXC_CRASH","signal":"SIGKILL"}"#,
        r#"{"namespace":"CODESIGNING","code":1,"indicator":"Invalid Page"}"#,
        "",
        "main",
    ));
    assert_eq!(signing.kind, Kind::CodeSigning);
}

#[test]
fn not_a_crash_report() {
    let e = analyze(b"report Chat.ips", &Options::default(), &NoSwift).unwrap_err();
    assert!(e.to_string().contains("not an .ips crash report"), "{e}");
    let jetsam = br#"{"bug_type":"298"}
{}"#;
    let e = analyze(jetsam, &Options::default(), &NoSwift).unwrap_err();
    assert!(e.to_string().contains("bug type 298"), "{e}");
}
