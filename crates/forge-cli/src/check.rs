//! `forge check`: the local equivalent of CI, in one command.

use std::path::Path;

use crate::commands::{require_workspace, run_checked, script, which_program};
use crate::runner::{Cmd, CommandRunner};

/// What the host can run, so steps that need missing tools are skipped (and
/// reported as skipped) instead of failing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Host {
    pub android_sdk: bool,
    pub java: bool,
    pub macos: bool,
    pub swift: bool,
}

impl Host {
    pub fn detect(runner: &dyn CommandRunner, root: &Path) -> Self {
        let android_sdk = ["ANDROID_HOME", "ANDROID_SDK_ROOT"]
            .iter()
            .any(|k| std::env::var_os(k).is_some_and(|v| !v.is_empty()))
            || std::fs::read_to_string(root.join(ANDROID_EXAMPLE).join("local.properties"))
                .is_ok_and(|p| p.lines().any(|l| l.trim_start().starts_with("sdk.dir")));
        Self {
            android_sdk,
            java: which_program(runner, "java"),
            macos: cfg!(target_os = "macos"),
            swift: which_program(runner, "swift"),
        }
    }
}

const ANDROID_EXAMPLE: &str = "examples/counter-android";
const CHAT_ANDROID_EXAMPLE: &str = "examples/chat-android";
const POCKET_LIBRARY_ANDROID_EXAMPLE: &str = "examples/pocket-library-android";

struct Step {
    name: &'static str,
    cmds: Vec<Cmd>,
    skip: Option<&'static str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Outcome {
    Passed,
    Failed,
    Skipped,
}

fn steps(root: &Path, host: Host) -> Vec<Step> {
    let cargo = |args: &[&str]| Cmd::new("cargo").args(args).cwd(root);
    let android_skip = match (host.java, host.android_sdk) {
        (false, _) => Some("java not found (JDK 17+)"),
        (_, false) => Some("Android SDK not found (set ANDROID_HOME)"),
        _ => None,
    };
    let swift_skip = match (host.macos, host.swift) {
        (false, _) => Some("XCTest host tests need macOS"),
        (_, false) => Some("swift not found"),
        _ => None,
    };
    let android_dir = root.join(ANDROID_EXAMPLE);
    let chat_android_dir = root.join(CHAT_ANDROID_EXAMPLE);
    let pocket_library_android_dir = root.join(POCKET_LIBRARY_ANDROID_EXAMPLE);
    vec![
        Step {
            name: "rustfmt",
            cmds: vec![cargo(&["fmt", "--all", "--", "--check"])],
            skip: None,
        },
        Step {
            name: "templates pin the workspace's UniFFI and verifier versions",
            cmds: vec![script(root, "check-template-sync.sh")],
            skip: None,
        },
        Step {
            name: "Xcode project layout",
            cmds: vec![
                script(root, "validate-xcode-project.sh"),
                script(root, "validate-xcode-project.sh")
                    .arg(root.join("examples/weather/Xcode/Weather.xcodeproj/project.pbxproj")),
                script(root, "validate-xcode-project.sh")
                    .arg(root.join("examples/chat/Xcode/Chat.xcodeproj/project.pbxproj")),
                script(root, "validate-xcode-project.sh").arg(
                    root.join(
                        "examples/pocket-library/Xcode/PocketLibrary.xcodeproj/project.pbxproj",
                    ),
                ),
                script(root, "validate-xcode-project.sh").arg(
                    root.join(
                        "templates/ios-app/Xcode/FORGE_PACKAGE_NAME.xcodeproj/project.pbxproj",
                    ),
                ),
            ],
            skip: None,
        },
        Step {
            name: "clippy",
            cmds: vec![cargo(&[
                "clippy",
                "--workspace",
                "--all-targets",
                "--",
                "-D",
                "warnings",
            ])],
            skip: None,
        },
        Step {
            name: "workspace tests",
            cmds: vec![cargo(&["test", "--workspace"])],
            skip: None,
        },
        Step {
            name: "Kotlin binding drift (Android examples)",
            cmds: vec![script(root, "check-bindings.sh")],
            skip: None,
        },
        Step {
            name: "app-owned crates (fmt, clippy, tests, binding drift)",
            cmds: vec![script(root, "check-app-crates.sh")],
            skip: None,
        },
        Step {
            name: "native rebuild regression",
            cmds: vec![script(root, "test-native-rebuild.sh")],
            skip: None,
        },
        Step {
            name: "Android JVM unit tests",
            cmds: vec![
                // The Chat tests also run against the real chat server.
                cargo(&[
                    "build",
                    "--manifest-path",
                    "examples/counter/rust/Cargo.toml",
                ]),
                cargo(&[
                    "build",
                    "--manifest-path",
                    "examples/chat-android/rust/Cargo.toml",
                ]),
                cargo(&[
                    "build",
                    "--manifest-path",
                    "examples/pocket-library-android/rust/Cargo.toml",
                ]),
                cargo(&["build", "-p", "chat-server"]),
                Cmd::new(android_dir.join("gradlew"))
                    .args([":app:testDebugUnitTest", "--no-daemon"])
                    .cwd(&android_dir),
                Cmd::new(chat_android_dir.join("gradlew"))
                    .args([":app:testDebugUnitTest", "--no-daemon"])
                    .cwd(&chat_android_dir),
                Cmd::new(pocket_library_android_dir.join("gradlew"))
                    .args([":app:testDebugUnitTest", "--no-daemon"])
                    .cwd(&pocket_library_android_dir),
            ],
            skip: android_skip,
        },
        Step {
            name: "Swift host tests (XCTest)",
            cmds: vec![
                script(root, "test-swift-host.sh").arg(root.join("examples/counter")),
                script(root, "test-swift-host.sh").arg(root.join("examples/weather")),
                script(root, "test-swift-host.sh").arg(root.join("examples/chat")),
                script(root, "test-swift-host.sh").arg(root.join("examples/pocket-library")),
            ],
            skip: swift_skip,
        },
    ]
}

/// Runs every local CI step, keeps going after a failure (unless `fail_fast`),
/// and prints a summary. Fails if any step failed; skipped steps do not fail.
pub fn check(
    runner: &dyn CommandRunner,
    root: &Path,
    host: Host,
    fail_fast: bool,
) -> Result<(), String> {
    require_workspace(root)?;
    let steps = steps(root, host);
    let total = steps.len();
    let mut results: Vec<(&str, Outcome, Option<String>)> = Vec::new();
    for (i, step) in steps.into_iter().enumerate() {
        println!("\n==> [{}/{total}] {}", i + 1, step.name);
        if let Some(reason) = step.skip {
            println!("skipped: {reason}");
            results.push((step.name, Outcome::Skipped, Some(reason.to_string())));
            continue;
        }
        match step
            .cmds
            .into_iter()
            .try_for_each(|c| run_checked(runner, c))
        {
            Ok(()) => results.push((step.name, Outcome::Passed, None)),
            Err(e) => {
                eprintln!("error: {e}");
                results.push((step.name, Outcome::Failed, Some(e)));
                if fail_fast {
                    break;
                }
            }
        }
    }

    println!("\n=== forge check summary ===");
    for (name, outcome, note) in &results {
        let label = match outcome {
            Outcome::Passed => "PASS",
            Outcome::Failed => "FAIL",
            Outcome::Skipped => "SKIP",
        };
        match (outcome, note) {
            (Outcome::Skipped, Some(n)) => println!("{label}  {name}: {n}"),
            _ => println!("{label}  {name}"),
        }
    }
    let count = |o| results.iter().filter(|r| r.1 == o).count();
    let (passed, failed, skipped) = (
        count(Outcome::Passed),
        count(Outcome::Failed),
        count(Outcome::Skipped),
    );
    let not_run = total - results.len();
    let mut line = format!("{passed} passed, {failed} failed, {skipped} skipped");
    if not_run > 0 {
        line.push_str(&format!(", {not_run} not run (--fail-fast)"));
    }
    println!("{line}");

    if failed == 0 {
        Ok(())
    } else {
        let names: Vec<&str> = results
            .iter()
            .filter(|r| r.1 == Outcome::Failed)
            .map(|r| r.0)
            .collect();
        Err(format!("forge check failed: {}", names.join(", ")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::workspace_root;
    use crate::runner::MockRunner;

    const ALL: Host = Host {
        android_sdk: true,
        java: true,
        macos: true,
        swift: true,
    };
    const BARE: Host = Host {
        android_sdk: false,
        java: false,
        macos: false,
        swift: false,
    };

    fn ok_runner(root: &Path) -> MockRunner {
        let mock = MockRunner::default();
        mock.respond("cargo", 0, "").respond("bash", 0, "");
        for example in [
            ANDROID_EXAMPLE,
            CHAT_ANDROID_EXAMPLE,
            POCKET_LIBRARY_ANDROID_EXAMPLE,
        ] {
            mock.respond(&root.join(example).join("gradlew").to_string_lossy(), 0, "");
        }
        mock
    }

    #[test]
    fn runs_the_full_ci_matrix_in_order() {
        let root = workspace_root();
        let mock = ok_runner(&root);
        check(&mock, &root, ALL, false).unwrap();
        let lines = mock.call_lines();
        let expected = [
            "cargo fmt --all -- --check",
            "check-template-sync.sh",
            "validate-xcode-project.sh",
            "Weather.xcodeproj/project.pbxproj",
            "Chat.xcodeproj/project.pbxproj",
            "PocketLibrary.xcodeproj/project.pbxproj",
            "FORGE_PACKAGE_NAME.xcodeproj/project.pbxproj",
            "cargo clippy --workspace --all-targets -- -D warnings",
            "cargo test --workspace",
            "check-bindings.sh",
            "check-app-crates.sh",
            "test-native-rebuild.sh",
            "examples/counter/rust/Cargo.toml",
            "examples/chat-android/rust/Cargo.toml",
            "examples/pocket-library-android/rust/Cargo.toml",
            "cargo build -p chat-server",
            "counter-android/gradlew :app:testDebugUnitTest --no-daemon",
            "chat-android/gradlew :app:testDebugUnitTest --no-daemon",
            "pocket-library-android/gradlew :app:testDebugUnitTest --no-daemon",
            "examples/counter",
            "examples/weather",
            "examples/chat",
            "examples/pocket-library",
        ];
        assert_eq!(lines.len(), expected.len(), "{lines:#?}");
        for (line, want) in lines.iter().zip(expected) {
            assert!(line.contains(want), "{line} should contain {want}");
        }
    }

    #[test]
    fn skips_platform_steps_without_their_tools() {
        let root = workspace_root();
        let mock = ok_runner(&root);
        check(&mock, &root, BARE, false).unwrap();
        let lines = mock.call_lines();
        assert!(!lines.iter().any(|l| l.contains("gradlew")));
        assert!(!lines.iter().any(|l| l.contains("test-swift-host")));
        assert!(!lines.iter().any(|l| l.contains("build -p chat-server")));
    }

    #[test]
    fn keeps_going_after_a_failure_and_reports_it() {
        let root = workspace_root();
        let mock = ok_runner(&root);
        mock.respond("cargo fmt", 1, "");
        let err = check(&mock, &root, BARE, false).unwrap_err();
        assert_eq!(err, "forge check failed: rustfmt");
        assert!(mock
            .call_lines()
            .iter()
            .any(|l| l == "cargo test --workspace"));
    }

    #[test]
    fn fail_fast_stops_at_the_first_failure() {
        let root = workspace_root();
        let mock = ok_runner(&root);
        mock.respond("cargo fmt", 1, "");
        check(&mock, &root, BARE, true).unwrap_err();
        assert_eq!(mock.calls().len(), 1);
    }

    #[test]
    fn refuses_outside_a_workspace() {
        let tmp = tempfile::tempdir().unwrap();
        let mock = MockRunner::default();
        let err = check(&mock, tmp.path(), ALL, false).unwrap_err();
        assert!(err.contains("not a Forge workspace"), "{err}");
        assert!(mock.calls().is_empty());
    }
}
