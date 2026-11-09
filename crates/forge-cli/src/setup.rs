//! `forge setup ios`: gets a Linux machine ready to build iOS apps, step by
//! step, and ends with the same readiness report as `forge doctor`.
//!
//! Setup gets you ready; doctor tells you what's wrong. Setup is safe to run
//! again: every step checks first and does nothing when the piece is
//! already there, so rerunning it is how you repair a machine.
//!
//! What it never does: download Xcode (Apple's licence and sign-in stay with
//! the person; Forge only uses a `.xip` they already have), see or keep the
//! Apple ID (xtool asks for it itself), or run `sudo` without asking.

use std::path::{Path, PathBuf};

use crate::doctor;
use crate::ios_env::{self, Auth, IOS_TARGET, MIN_IOS_SDK_MAJOR};
use crate::runner::{Cmd, CommandRunner};

/// The Swift release the docs and CI use (docs/linux-setup.md).
const SWIFT_RELEASE: &str = "6.2.3";

/// What `forge setup ios` was asked to do.
pub struct Options {
    /// An Xcode.xip (or Xcode.app) to install the Darwin SDK from.
    pub xip: Option<PathBuf>,
    /// Install the Darwin SDK from the .xip even when a compatible one is
    /// already installed.
    pub reinstall_sdk: bool,
    /// Answer yes to every question (for scripts). Still never downloads
    /// Xcode or signs in to Apple on the person's behalf.
    pub yes: bool,
    /// A person is at the terminal: questions can be asked, and xtool can
    /// prompt for the Apple ID.
    pub interactive: bool,
}

/// How one step ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// It was already in place.
    Ready(String),
    /// Setup did it just now.
    Done(String),
    /// It still needs the person: what to do.
    Todo(String),
}

/// One step's result, for the summary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    pub name: &'static str,
    pub outcome: Outcome,
}

/// Everything setup needs from outside, so tests can stand in for it.
pub struct Host<'a> {
    pub runner: &'a dyn CommandRunner,
    /// The Forge checkout (scripts, and the pinned Rust toolchain).
    pub root: &'a Path,
    pub env: &'a dyn Fn(&str) -> Option<String>,
    /// Asks a yes/no question; only called when `interactive` and not `yes`.
    pub ask: &'a dyn Fn(&str) -> bool,
    pub os: &'a str,
    pub arch: &'a str,
}

/// Runs every step and prints the summary and readiness report. Fails when
/// something still needs the person, so scripts can tell.
pub fn setup_ios(host: &Host, opts: &Options) -> Result<(), String> {
    if host.os != "linux" {
        return Err(
            "`forge setup ios` sets up a Linux machine. On a Mac, install Xcode 26 from the App Store; `forge doctor` then shows what else is missing (docs/ios-xcode.md)."
                .into(),
        );
    }
    let steps = run_steps(host, opts);
    print!("{}", summary(&steps));
    let report = doctor::report(host.runner, host.root, host.env);
    let flows = doctor::workflows(&report);
    for w in flows
        .iter()
        .filter(|w| w.name.starts_with("iOS") || w.name.starts_with("Run on"))
    {
        println!(
            "{}: {}",
            w.name,
            if w.ready { "ready" } else { "not ready" }
        );
    }
    let ios = flows.iter().find(|w| w.name.starts_with("iOS from Linux"));
    verdict(
        &steps,
        ios.is_some_and(|w| w.ready),
        ios.map(|w| w.fixes.as_slice()).unwrap_or_default(),
    )?;
    println!(
        "\nNext: create an app with `forge init MyApp --minimal --bundle-id com.yourname.myapp`, connect your iPhone, and run `forge run ios` in it (docs/getting-started.md)."
    );
    Ok(())
}

/// Setup succeeds only when no step needs the person and `forge doctor`
/// agrees that iOS builds from Linux are ready: a step can't pass on its
/// own say-so.
fn verdict(steps: &[Step], ios_ready: bool, ios_fixes: &[String]) -> Result<(), String> {
    let todo = steps
        .iter()
        .filter(|s| matches!(s.outcome, Outcome::Todo(_)))
        .count();
    if todo > 0 {
        return Err(format!(
            "{todo} step{} still need{} you (marked ! above). Run `forge setup ios` again afterwards.",
            if todo == 1 { "" } else { "s" },
            if todo == 1 { "s" } else { "" }
        ));
    }
    if !ios_ready {
        let fixes = if ios_fixes.is_empty() {
            "run `forge doctor` to see why".to_string()
        } else {
            ios_fixes.join("; ")
        };
        return Err(format!(
            "every step finished, but `forge doctor` says iOS builds from Linux aren't ready yet: {fixes}. Fix that, then run `forge setup ios` again."
        ));
    }
    Ok(())
}

/// The steps in order. Later steps that depend on an unfinished earlier one
/// say so instead of failing.
pub fn run_steps(host: &Host, opts: &Options) -> Vec<Step> {
    let mut steps = vec![
        rust_target(host),
        system_packages(host, opts),
        xtool(host, opts),
        swift(host, opts),
    ];
    let xtool_ok = matches!(steps[2].outcome, Outcome::Ready(_) | Outcome::Done(_));
    let swift_ok = matches!(steps[3].outcome, Outcome::Ready(_) | Outcome::Done(_));
    let waiting_for = match (xtool_ok, swift_ok) {
        (true, true) => None,
        (false, true) => Some("xtool"),
        (true, false) => Some("Swift"),
        (false, false) => Some("xtool and Swift"),
    };
    steps.push(darwin_sdk(host, opts, waiting_for));
    steps.push(apple_sign_in(host, opts, xtool_ok));
    steps
}

/// The step list, as printed before the readiness lines.
pub fn summary(steps: &[Step]) -> String {
    let mut out = String::from("\n[forge] iOS setup:\n");
    for s in steps {
        let (mark, text) = match &s.outcome {
            Outcome::Ready(t) => ("✓", t),
            Outcome::Done(t) => ("✓", t),
            Outcome::Todo(t) => ("!", t),
        };
        out.push_str(&format!("  {mark} {}: {text}\n", s.name));
    }
    out.push('\n');
    out
}

fn confirm(host: &Host, opts: &Options, question: &str) -> bool {
    opts.yes || (opts.interactive && (host.ask)(question))
}

fn succeeded(host: &Host, cmd: &Cmd) -> bool {
    println!("[forge] {}", cmd.display());
    host.runner.stream(cmd).is_ok_and(|s| s.success())
}

/// The iOS Rust target, added to the toolchain the checkout pins
/// (rust-toolchain.toml), which is the one Forge builds with. Run from
/// anywhere else, `rustup target add` changes the default toolchain instead.
fn rust_target(host: &Host) -> Step {
    let name = "Rust iOS target";
    let installed = host
        .runner
        .capture(
            &Cmd::new("rustup")
                .args(["target", "list", "--installed"])
                .cwd(host.root),
        )
        .ok()
        .filter(|o| o.status.success())
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .any(|l| l.trim() == IOS_TARGET)
        });
    match installed {
        Some(true) => Step {
            name,
            outcome: Outcome::Ready(format!("{IOS_TARGET} is installed")),
        },
        None => Step {
            name,
            outcome: Outcome::Todo(
                "install Rust with rustup (https://rustup.rs), then run `forge setup ios` again"
                    .into(),
            ),
        },
        Some(false) => {
            let cmd = Cmd::new("rustup")
                .args(["target", "add", IOS_TARGET])
                .cwd(host.root);
            if succeeded(host, &cmd) {
                Step {
                    name,
                    outcome: Outcome::Done(format!("added {IOS_TARGET}")),
                }
            } else {
                Step {
                    name,
                    outcome: Outcome::Todo(format!(
                        "`rustup target add {IOS_TARGET}` failed; run it inside {} and read its error",
                        host.root.display()
                    )),
                }
            }
        }
    }
}

/// Debian and Ubuntu packages: usbmuxd and libimobiledevice talk to the
/// iPhone over USB; FUSE lets the xtool AppImage start.
const PACKAGES: [(&str, &[&str]); 3] = [
    ("usbmuxd", &["usbmuxd"]),
    ("libimobiledevice-utils", &["libimobiledevice-utils"]),
    // Either FUSE provides fusermount; fuse3 is what current Ubuntu ships.
    ("fuse3", &["fuse3", "fuse"]),
];

fn package_installed(host: &Host, package: &str) -> bool {
    host.runner
        .capture(&Cmd::new("dpkg-query").args(["-W", "-f=${Status}", package]))
        .ok()
        .filter(|o| o.status.success())
        .is_some_and(|o| String::from_utf8_lossy(&o.stdout).contains("install ok installed"))
}

fn system_packages(host: &Host, opts: &Options) -> Step {
    let name = "USB and FUSE packages";
    let has_dpkg = host
        .runner
        .capture(&Cmd::new("dpkg-query").arg("--version"))
        .is_ok_and(|o| o.status.success());
    if !has_dpkg {
        return Step {
            name,
            outcome: Outcome::Todo(
                "install usbmuxd, libimobiledevice and FUSE with your distribution's package manager (Forge only installs them on Debian and Ubuntu)"
                    .into(),
            ),
        };
    }
    let missing: Vec<&str> = PACKAGES
        .iter()
        .filter(|(_, any_of)| !any_of.iter().any(|p| package_installed(host, p)))
        .map(|(install, _)| *install)
        .collect();
    if missing.is_empty() {
        return Step {
            name,
            outcome: Outcome::Ready("usbmuxd, libimobiledevice and FUSE are installed".into()),
        };
    }
    let list = missing.join(" ");
    let command = format!("sudo apt-get update && sudo apt-get install -y {list}");
    if !confirm(
        host,
        opts,
        &format!("Install {list}? This runs `{command}` and asks for your password."),
    ) {
        return Step {
            name,
            outcome: Outcome::Todo(format!("run `{command}`")),
        };
    }
    let update = Cmd::new("sudo").args(["apt-get", "update"]);
    let install = Cmd::new("sudo")
        .args(["apt-get", "install", "-y"])
        .args(&missing);
    if succeeded(host, &update) && succeeded(host, &install) {
        Step {
            name,
            outcome: Outcome::Done(format!("installed {list}")),
        }
    } else {
        Step {
            name,
            outcome: Outcome::Todo(format!(
                "`{command}` failed; run it yourself and read its error"
            )),
        }
    }
}

/// What `xtool --version` says about the installed xtool.
enum XtoolState {
    Missing,
    /// The AppImage can't start without FUSE.
    NeedsFuse,
    Version(String),
}

fn xtool_state(host: &Host) -> XtoolState {
    match host.runner.capture(&Cmd::new("xtool").arg("--version")) {
        Err(_) => XtoolState::Missing,
        Ok(out) => {
            let text = format!(
                "{}{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            );
            if text.contains("fusermount") || text.contains("libfuse") {
                XtoolState::NeedsFuse
            } else if !out.status.success() {
                XtoolState::Missing
            } else {
                let line = text
                    .lines()
                    .map(str::trim)
                    .find(|l| !l.is_empty())
                    .unwrap_or("");
                XtoolState::Version(
                    line.strip_prefix("xtool")
                        .unwrap_or(line)
                        .trim()
                        .to_string(),
                )
            }
        }
    }
}

fn is_pinned(found: &str) -> bool {
    let expected = doctor::pinned_xtool_version();
    found == expected || found.strip_prefix(expected) == Some("-dirty")
}

fn xtool(host: &Host, opts: &Options) -> Step {
    let name = "xtool";
    let pinned = doctor::pinned_xtool_version();
    let why = match xtool_state(host) {
        XtoolState::Version(v) if is_pinned(&v) => {
            return Step {
                name,
                outcome: Outcome::Ready(format!("xtool {v}")),
            }
        }
        XtoolState::NeedsFuse => {
            return Step {
                name,
                outcome: Outcome::Todo(
                    "xtool can't start without FUSE: run `sudo apt-get install fuse3`".into(),
                ),
            }
        }
        XtoolState::Version(v) => {
            format!("xtool {v} is installed, but Forge is tested with {pinned}")
        }
        XtoolState::Missing => "xtool isn't installed".to_string(),
    };
    if host.arch != "x86_64" {
        return Step {
            name,
            outcome: Outcome::Todo(format!(
                "{why}, and there's no pinned build for {}: build the fork's `ixos` branch (docs/linux-setup.md#install-xtool)",
                host.arch
            )),
        };
    }
    let script = host.root.join("scripts/install-xtool.sh");
    if !confirm(
        host,
        opts,
        &format!("{why}. Install xtool {pinned} to /usr/local/bin (checksum-verified; may ask for your password)?"),
    ) {
        return Step {
            name,
            outcome: Outcome::Todo(format!("{why}: run `{}`", script.display())),
        };
    }
    if !succeeded(host, &Cmd::new("bash").arg(&script)) {
        return Step {
            name,
            outcome: Outcome::Todo(format!(
                "`{}` failed; read its error above",
                script.display()
            )),
        };
    }
    match xtool_state(host) {
        XtoolState::Version(v) if is_pinned(&v) => Step {
            name,
            outcome: Outcome::Done(format!("installed xtool {v}")),
        },
        XtoolState::Version(v) => Step {
            name,
            outcome: Outcome::Todo(format!(
                "the install script finished, but the `xtool` on your PATH is still {v}, not {pinned}: run `which -a xtool` and remove the older one"
            )),
        },
        XtoolState::NeedsFuse => Step {
            name,
            outcome: Outcome::Todo(
                "xtool is installed but can't start without FUSE: run `sudo apt-get install fuse3`"
                    .into(),
            ),
        },
        XtoolState::Missing => Step {
            name,
            outcome: Outcome::Todo(
                "xtool was installed to /usr/local/bin but doesn't run: check that /usr/local/bin is on your PATH"
                    .into(),
            ),
        },
    }
}

/// The Swift tarball for Ubuntu 24.04 on x86_64, the setup docs/linux-setup.md
/// describes. Other systems get a pointer to swift.org instead.
fn swift_install_script() -> String {
    let dir = format!("swift-{SWIFT_RELEASE}-RELEASE-ubuntu24.04");
    format!(
        "set -e; curl -fL -o /tmp/swift.tar.gz https://download.swift.org/swift-{SWIFT_RELEASE}-release/ubuntu2404/swift-{SWIFT_RELEASE}-RELEASE/{dir}.tar.gz; \
         sudo tar -xzf /tmp/swift.tar.gz -C /usr/local; \
         sudo ln -sf /usr/local/{dir}/usr/bin/swift /usr/local/bin/swift; \
         sudo apt-get install -y libncurses6; rm -f /tmp/swift.tar.gz"
    )
}

fn is_ubuntu_2404(host: &Host) -> bool {
    host.runner
        .capture(&Cmd::new("cat").arg("/etc/os-release"))
        .ok()
        .is_some_and(|o| {
            let text = String::from_utf8_lossy(&o.stdout);
            text.lines().any(|l| l.trim() == "ID=ubuntu")
                && text.lines().any(|l| l.trim() == "VERSION_ID=\"24.04\"")
        })
}

fn swift(host: &Host, opts: &Options) -> Step {
    let name = "Swift";
    match ios_env::swift_version(host.runner) {
        Some((major, minor)) if (major, minor) >= (6, 2) => Step {
            name,
            outcome: Outcome::Ready(format!("Swift {major}.{minor}")),
        },
        Some((major, minor)) => Step {
            name,
            outcome: Outcome::Todo(format!(
                "Swift {major}.{minor} is too old: install Swift {SWIFT_RELEASE} (docs/linux-setup.md#install-swift-ubuntu-2404-example)"
            )),
        },
        None if host.arch == "x86_64" && is_ubuntu_2404(host) => {
            if !confirm(
                host,
                opts,
                &format!("Swift isn't installed. Download Swift {SWIFT_RELEASE} from swift.org (about 900 MB) and install it to /usr/local? It asks for your password."),
            ) {
                return Step {
                    name,
                    outcome: Outcome::Todo(format!(
                        "Swift isn't installed: install Swift {SWIFT_RELEASE} (docs/linux-setup.md#install-swift-ubuntu-2404-example)"
                    )),
                };
            }
            if succeeded(host, &Cmd::new("bash").args(["-c", &swift_install_script()]))
                && ios_env::swift_version(host.runner).is_some_and(|v| v >= (6, 2))
            {
                Step {
                    name,
                    outcome: Outcome::Done(format!("installed Swift {SWIFT_RELEASE}")),
                }
            } else {
                Step {
                    name,
                    outcome: Outcome::Todo(
                        "installing Swift failed; follow docs/linux-setup.md#install-swift-ubuntu-2404-example"
                            .into(),
                    ),
                }
            }
        }
        None => Step {
            name,
            outcome: Outcome::Todo(format!(
                "Swift isn't installed: install Swift {SWIFT_RELEASE} for your system from https://www.swift.org/install/linux/"
            )),
        },
    }
}

/// An Xcode.xip in ~/Downloads, newest first, when none was given.
fn find_xip(host: &Host) -> Option<PathBuf> {
    let home = (host.env)("HOME").filter(|h| !h.is_empty())?;
    let downloads = Path::new(&home).join("Downloads");
    let mut found: Vec<(std::time::SystemTime, PathBuf)> = std::fs::read_dir(downloads)
        .ok()?
        .flatten()
        .filter(|e| {
            let n = e.file_name().to_string_lossy().to_lowercase();
            n.starts_with("xcode") && n.ends_with(".xip")
        })
        .map(|e| {
            let modified = e
                .metadata()
                .and_then(|m| m.modified())
                .unwrap_or(std::time::UNIX_EPOCH);
            (modified, e.path())
        })
        .collect();
    found.sort();
    found.pop().map(|(_, p)| p)
}

const GET_XCODE: &str = "download Xcode 26 as a .xip from https://developer.apple.com/download/all/ (sign in with any Apple ID; it is several GB), then run `forge setup ios --xip <path to Xcode.xip>`";

/// `waiting_for` names the earlier steps the SDK install needs, if unfinished.
fn darwin_sdk(host: &Host, opts: &Options, waiting_for: Option<&str>) -> Step {
    let name = "Darwin SDK";
    let installed = ios_env::darwin_swift_sdk(host.runner) == Some(true);
    let version = installed
        .then(|| ios_env::darwin_ios_sdk_version(host.runner, host.env))
        .flatten();
    let too_old = version
        .as_deref()
        .and_then(ios_env::sdk_major)
        .is_some_and(|m| m < MIN_IOS_SDK_MAJOR);
    let compatible = version.as_deref().is_some_and(sdk_is_compatible);
    if let (true, true, false, Some(v)) = (installed, compatible, opts.reinstall_sdk, &version) {
        let note = if opts.xip.is_some() {
            " (already installed, so the .xip wasn't used; --reinstall-sdk replaces it)"
        } else {
            ""
        };
        return Step {
            name,
            outcome: Outcome::Ready(format!("iOS {v} SDK{note}")),
        };
    }
    let unknown = if installed && !compatible && !too_old {
        match &version {
            Some(v) => format!(
                "a Darwin SDK is installed, but Forge can't tell which iOS it is for (\"{v}\"); "
            ),
            None => "a Darwin SDK is installed, but Forge can't read its iOS version; ".to_string(),
        }
    } else {
        String::new()
    };
    if let Some(missing) = waiting_for {
        return Step {
            name,
            outcome: Outcome::Todo(format!(
                "needs {missing} first (above); then run `forge setup ios` again"
            )),
        };
    }
    let old = if !unknown.is_empty() {
        unknown
    } else if too_old {
        format!(
            "the installed SDK is iOS {}, which fails to link with Swift 6.2; ",
            version.as_deref().unwrap_or("?")
        )
    } else {
        String::new()
    };
    let xip = match &opts.xip {
        Some(p) if p.exists() => p.clone(),
        Some(p) => {
            return Step {
                name,
                outcome: Outcome::Todo(format!("{} doesn't exist; {GET_XCODE}", p.display())),
            }
        }
        None => match find_xip(host) {
            Some(p)
                if confirm(
                    host,
                    opts,
                    &format!(
                        "Install the Darwin SDK from {}? It takes several minutes.",
                        p.display()
                    ),
                ) =>
            {
                p
            }
            Some(p) => {
                return Step {
                    name,
                    outcome: Outcome::Todo(format!(
                        "{old}run `forge setup ios --xip {}`",
                        p.display()
                    )),
                }
            }
            None => {
                return Step {
                    name,
                    outcome: Outcome::Todo(format!("{old}{GET_XCODE}")),
                }
            }
        },
    };
    println!(
        "[forge] Installing the Darwin SDK from {} (several minutes)",
        xip.display()
    );
    if !succeeded(host, &Cmd::new("xtool").args(["sdk", "install"]).arg(&xip)) {
        return Step {
            name,
            outcome: Outcome::Todo(format!(
                "`xtool sdk install {}` failed; read its error above. Is it a complete Xcode 26 download?",
                xip.display()
            )),
        };
    }
    match ios_env::darwin_ios_sdk_version(host.runner, host.env) {
        Some(v) if ios_env::sdk_major(&v).is_some_and(|m| m < MIN_IOS_SDK_MAJOR) => Step {
            name,
            outcome: Outcome::Todo(format!(
                "that Xcode has the iOS {v} SDK, which fails to link with Swift 6.2: use Xcode 26's .xip"
            )),
        },
        Some(v) if sdk_is_compatible(&v) => Step {
            name,
            outcome: Outcome::Done(format!("installed the iOS {v} SDK")),
        },
        Some(v) => Step {
            name,
            outcome: Outcome::Todo(format!(
                "`xtool sdk install` finished, but Forge can't tell which iOS the SDK is for (\"{v}\"): run `forge doctor`, and use Xcode 26's .xip"
            )),
        },
        None => Step {
            name,
            outcome: Outcome::Todo(
                "`xtool sdk install` finished, but Forge can't find the iOS SDK it should have installed: run `swift sdk list` and `forge doctor`".into(),
            ),
        },
    }
}

/// Only a version Forge can read and that is new enough counts: an
/// unreadable one is never taken as compatible.
fn sdk_is_compatible(version: &str) -> bool {
    ios_env::sdk_major(version).is_some_and(|m| m >= MIN_IOS_SDK_MAJOR)
}

fn apple_sign_in(host: &Host, opts: &Options, xtool_ready: bool) -> Step {
    let name = "Apple sign-in";
    if !xtool_ready {
        return Step {
            name,
            outcome: Outcome::Todo("needs xtool first (above)".into()),
        };
    }
    match ios_env::xtool_auth(host.runner) {
        Some(Auth::LoggedIn) => Step {
            name,
            outcome: Outcome::Ready("xtool is signed in".into()),
        },
        _ if !opts.interactive => Step {
            name,
            outcome: Outcome::Todo(
                "run `xtool auth login` in a terminal and sign in with your Apple ID".into(),
            ),
        },
        _ => {
            println!(
                "[forge] xtool now asks for your Apple ID. A free one works. Forge doesn't see or keep it."
            );
            if succeeded(host, &Cmd::new("xtool").args(["auth", "login"]))
                && ios_env::xtool_auth(host.runner) == Some(Auth::LoggedIn)
            {
                Step {
                    name,
                    outcome: Outcome::Done("signed in".into()),
                }
            } else {
                Step {
                    name,
                    outcome: Outcome::Todo("sign-in didn't finish: run `xtool auth login`".into()),
                }
            }
        }
    }
}

/// Asks on the terminal; anything but y or yes is no.
pub fn ask_terminal(question: &str) -> bool {
    use std::io::Write;
    print!("{question} [y/N] ");
    let _ = std::io::stdout().flush();
    let mut line = String::new();
    if std::io::stdin().read_line(&mut line).is_err() {
        return false;
    }
    matches!(line.trim().to_lowercase().as_str(), "y" | "yes")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::MockRunner;

    const INSTALLED: &str = "install ok installed";

    fn host<'a>(
        mock: &'a MockRunner,
        root: &'a Path,
        env: &'a dyn Fn(&str) -> Option<String>,
        ask: &'a dyn Fn(&str) -> bool,
    ) -> Host<'a> {
        Host {
            runner: mock,
            root,
            env,
            ask,
            os: "linux",
            arch: "x86_64",
        }
    }

    /// A machine where everything is already in place.
    fn ready_machine() -> MockRunner {
        let mock = MockRunner::default();
        mock.respond("rustup target list", 0, "aarch64-apple-ios\n")
            .respond("dpkg-query", 0, INSTALLED)
            .respond(
                "xtool --version",
                0,
                &format!("xtool {}\n", doctor::pinned_xtool_version()),
            )
            .respond(
                "swift --version",
                0,
                "Swift version 6.2.3 (swift-6.2.3-RELEASE)\n",
            )
            .respond("swift sdk list", 0, "darwin\n")
            .respond(
                "swift sdk configure",
                0,
                "sdkRootPath: /nonexistent/iPhoneOS26.0.sdk\n",
            )
            .respond("xtool auth status", 0, "Logged in.\n");
        mock
    }

    fn opts(interactive: bool) -> Options {
        Options {
            xip: None,
            reinstall_sdk: false,
            yes: false,
            interactive,
        }
    }

    fn outcome<'s>(steps: &'s [Step], name: &str) -> &'s Outcome {
        &steps.iter().find(|s| s.name == name).expect(name).outcome
    }

    #[test]
    fn a_ready_machine_changes_nothing() {
        let mock = ready_machine();
        let root = PathBuf::from("/forge");
        let env = |_: &str| None;
        let ask = |_: &str| panic!("asked on a ready machine");
        let steps = run_steps(&host(&mock, &root, &env, &ask), &opts(true));
        assert!(
            steps.iter().all(|s| matches!(s.outcome, Outcome::Ready(_))),
            "{steps:?}"
        );
        let calls = mock.call_lines();
        for changes in [
            "sudo",
            "target add",
            "sdk install",
            "auth login",
            "install-xtool",
            "bash -c",
        ] {
            assert!(
                calls.iter().all(|c| !c.contains(changes)),
                "{changes}: {calls:?}"
            );
        }
    }

    #[test]
    fn rust_target_is_added_to_the_pinned_toolchain() {
        let mock = ready_machine();
        mock.respond("rustup target list", 0, "x86_64-unknown-linux-gnu\n")
            .respond("rustup target add", 0, "");
        let root = PathBuf::from("/forge");
        let env = |_: &str| None;
        let ask = |_: &str| false;
        let steps = run_steps(&host(&mock, &root, &env, &ask), &opts(false));
        assert!(matches!(
            outcome(&steps, "Rust iOS target"),
            Outcome::Done(_)
        ));
        let add = mock
            .calls()
            .into_iter()
            .find(|c| c.display().starts_with("rustup target add"))
            .unwrap();
        assert_eq!(add.cwd.as_deref(), Some(root.as_path()));
    }

    #[test]
    fn sudo_runs_only_after_a_yes() {
        let mock = ready_machine();
        mock.respond("dpkg-query --version", 0, "").respond(
            "dpkg-query -W -f=${Status} fuse",
            1,
            "",
        );
        let root = PathBuf::from("/forge");
        let env = |_: &str| None;
        let no = |_: &str| false;
        let steps = run_steps(&host(&mock, &root, &env, &no), &opts(true));
        match outcome(&steps, "USB and FUSE packages") {
            Outcome::Todo(t) => assert!(t.contains("sudo apt-get install -y fuse3"), "{t}"),
            other => panic!("{other:?}"),
        }
        assert!(mock.call_lines().iter().all(|c| !c.starts_with("sudo")));

        let mock = ready_machine();
        mock.respond("dpkg-query --version", 0, "")
            .respond("dpkg-query -W -f=${Status} fuse", 1, "")
            .respond("sudo apt-get", 0, "");
        let yes = |q: &str| {
            assert!(q.contains("fuse3"), "{q}");
            true
        };
        let steps = run_steps(&host(&mock, &root, &env, &yes), &opts(true));
        assert!(matches!(
            outcome(&steps, "USB and FUSE packages"),
            Outcome::Done(_)
        ));
        assert!(mock
            .call_lines()
            .iter()
            .any(|c| c == "sudo apt-get install -y fuse3"));
    }

    #[test]
    fn without_a_terminal_nothing_is_installed_unless_yes() {
        let mock = ready_machine();
        mock.respond("dpkg-query --version", 0, "")
            .respond("dpkg-query -W -f=${Status} usbmuxd", 1, "")
            .respond("sudo", 0, "");
        let root = PathBuf::from("/forge");
        let env = |_: &str| None;
        let ask = |_: &str| panic!("asked without a terminal");
        run_steps(&host(&mock, &root, &env, &ask), &opts(false));
        assert!(mock.call_lines().iter().all(|c| !c.starts_with("sudo")));

        let yes = Options {
            yes: true,
            ..opts(false)
        };
        run_steps(&host(&mock, &root, &env, &ask), &yes);
        assert!(mock
            .call_lines()
            .iter()
            .any(|c| c == "sudo apt-get install -y usbmuxd"));
    }

    #[test]
    fn xtool_without_fuse_points_at_fuse_not_the_install_script() {
        let mock = ready_machine();
        mock.respond(
            "xtool --version",
            0,
            "Error: No suitable fusermount binary found on the $PATH\n",
        );
        let root = PathBuf::from("/forge");
        let env = |_: &str| None;
        let ask = |_: &str| true;
        let steps = run_steps(&host(&mock, &root, &env, &ask), &opts(true));
        match outcome(&steps, "xtool") {
            Outcome::Todo(t) => assert!(t.contains("fuse3"), "{t}"),
            other => panic!("{other:?}"),
        }
        assert!(mock
            .call_lines()
            .iter()
            .all(|c| !c.contains("install-xtool")));
        // Sign-in waits for xtool rather than failing.
        assert!(
            matches!(outcome(&steps, "Apple sign-in"), Outcome::Todo(t) if t.contains("first"))
        );
        assert!(mock.call_lines().iter().all(|c| c != "xtool auth login"));
    }

    #[test]
    fn missing_xtool_is_installed_from_the_pinned_script() {
        let mock = ready_machine();
        mock.respond("xtool --version", 127, "");
        // The mock answers by prefix, so after the script the same probe
        // still says missing; what matters here is that the script ran.
        mock.respond("bash /forge/scripts/install-xtool.sh", 0, "");
        let root = PathBuf::from("/forge");
        let env = |_: &str| None;
        let ask = |_: &str| true;
        run_steps(&host(&mock, &root, &env, &ask), &opts(true));
        assert!(mock
            .call_lines()
            .iter()
            .any(|c| c == "bash /forge/scripts/install-xtool.sh"));
    }

    #[test]
    fn no_xip_says_where_to_get_xcode_and_downloads_nothing() {
        let mock = ready_machine();
        mock.respond("swift sdk list", 0, "\n");
        let root = PathBuf::from("/forge");
        let home = tempfile::tempdir().unwrap();
        let home_path = home.path().to_string_lossy().into_owned();
        let env = move |k: &str| (k == "HOME").then(|| home_path.clone());
        let ask = |_: &str| true;
        let steps = run_steps(&host(&mock, &root, &env, &ask), &opts(true));
        match outcome(&steps, "Darwin SDK") {
            Outcome::Todo(t) => {
                assert!(t.contains("developer.apple.com"), "{t}");
                assert!(t.contains("--xip"), "{t}");
            }
            other => panic!("{other:?}"),
        }
        assert!(mock.call_lines().iter().all(|c| !c.contains("sdk install")));
    }

    #[test]
    fn a_xip_in_downloads_is_offered_and_installed() {
        let mock = ready_machine();
        mock.respond("swift sdk list", 0, "\n")
            .respond("xtool sdk install", 0, "");
        let root = PathBuf::from("/forge");
        let home = tempfile::tempdir().unwrap();
        std::fs::create_dir(home.path().join("Downloads")).unwrap();
        let xip = home.path().join("Downloads/Xcode_26.xip");
        std::fs::write(&xip, b"").unwrap();
        let home_path = home.path().to_string_lossy().into_owned();
        let env = move |k: &str| (k == "HOME").then(|| home_path.clone());
        let ask = |q: &str| q.contains("Xcode_26.xip");
        run_steps(&host(&mock, &root, &env, &ask), &opts(true));
        let want = format!("xtool sdk install {}", xip.display());
        assert!(mock.call_lines().contains(&want), "{:?}", mock.call_lines());
    }

    #[test]
    fn an_old_sdk_asks_for_xcode_26() {
        let mock = ready_machine();
        mock.respond(
            "swift sdk configure",
            0,
            "sdkRootPath: /nonexistent/iPhoneOS18.0.sdk\n",
        );
        let root = PathBuf::from("/forge");
        let env = |_: &str| None;
        let ask = |_: &str| true;
        let steps = run_steps(&host(&mock, &root, &env, &ask), &opts(true));
        match outcome(&steps, "Darwin SDK") {
            Outcome::Todo(t) => assert!(t.contains("iOS 18.0") && t.contains("Xcode 26"), "{t}"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn sign_in_is_left_to_xtool_and_the_person() {
        let mock = ready_machine();
        mock.respond("xtool auth status", 0, "Logged out\n");
        let root = PathBuf::from("/forge");
        let env = |_: &str| None;
        let ask = |_: &str| true;
        let steps = run_steps(&host(&mock, &root, &env, &ask), &opts(false));
        match outcome(&steps, "Apple sign-in") {
            Outcome::Todo(t) => assert!(t.contains("xtool auth login"), "{t}"),
            other => panic!("{other:?}"),
        }
        // No terminal: xtool isn't started to prompt for a password.
        assert!(mock.call_lines().iter().all(|c| c != "xtool auth login"));

        mock.respond("xtool auth login", 0, "");
        run_steps(&host(&mock, &root, &env, &ask), &opts(true));
        assert!(mock.call_lines().iter().any(|c| c == "xtool auth login"));
    }

    #[test]
    fn swift_is_installed_only_on_ubuntu_24_04_after_a_yes() {
        let mock = ready_machine();
        mock.respond("swift --version", 127, "").respond(
            "cat /etc/os-release",
            0,
            "ID=fedora\nVERSION_ID=\"41\"\n",
        );
        let root = PathBuf::from("/forge");
        let env = |_: &str| None;
        let ask = |_: &str| panic!("offered to install Swift on Fedora");
        let steps = run_steps(&host(&mock, &root, &env, &ask), &opts(true));
        match outcome(&steps, "Swift") {
            Outcome::Todo(t) => assert!(t.contains("swift.org/install"), "{t}"),
            other => panic!("{other:?}"),
        }

        mock.respond(
            "cat /etc/os-release",
            0,
            "ID=ubuntu\nVERSION_ID=\"24.04\"\n",
        );
        let no = |q: &str| {
            assert!(q.contains("Swift"), "{q}");
            false
        };
        run_steps(&host(&mock, &root, &env, &no), &opts(true));
        assert!(mock.call_lines().iter().all(|c| !c.starts_with("bash -c")));
    }

    #[test]
    fn not_linux_is_refused_with_the_mac_route() {
        let mock = MockRunner::default();
        let root = PathBuf::from("/forge");
        let env = |_: &str| None;
        let ask = |_: &str| false;
        let mut h = host(&mock, &root, &env, &ask);
        h.os = "macos";
        let err = setup_ios(&h, &opts(true)).unwrap_err();
        assert!(err.contains("Xcode 26"), "{err}");
        assert!(mock.calls().is_empty());
    }

    #[test]
    fn summary_marks_what_needs_the_person() {
        let text = summary(&[
            Step {
                name: "xtool",
                outcome: Outcome::Ready("xtool 1".into()),
            },
            Step {
                name: "Darwin SDK",
                outcome: Outcome::Todo("download Xcode".into()),
            },
        ]);
        assert!(text.contains("  ✓ xtool: xtool 1"), "{text}");
        assert!(text.contains("  ! Darwin SDK: download Xcode"), "{text}");
    }
    #[test]
    fn a_rerun_with_xip_keeps_a_compatible_sdk() {
        let mock = ready_machine();
        let root = PathBuf::from("/forge");
        let dir = tempfile::tempdir().unwrap();
        let xip = dir.path().join("Xcode_26.xip");
        std::fs::write(&xip, b"").unwrap();
        let env = |_: &str| None;
        let ask = |_: &str| true;
        let with_xip = Options {
            xip: Some(xip.clone()),
            ..opts(true)
        };
        let steps = run_steps(&host(&mock, &root, &env, &ask), &with_xip);
        match outcome(&steps, "Darwin SDK") {
            Outcome::Ready(t) => assert!(
                t.contains("iOS 26.0") && t.contains("--reinstall-sdk"),
                "{t}"
            ),
            other => panic!("{other:?}"),
        }
        assert!(mock.call_lines().iter().all(|c| !c.contains("sdk install")));

        mock.respond("xtool sdk install", 0, "");
        let reinstall = Options {
            reinstall_sdk: true,
            ..with_xip
        };
        let steps = run_steps(&host(&mock, &root, &env, &ask), &reinstall);
        assert!(
            matches!(outcome(&steps, "Darwin SDK"), Outcome::Done(_)),
            "{steps:?}"
        );
        assert!(mock.call_lines().iter().any(|c| c.contains("sdk install")));
    }

    #[test]
    fn an_sdk_whose_version_cant_be_read_is_not_counted_as_installed() {
        let mock = ready_machine();
        mock.respond("swift sdk list", 0, "\n")
            .respond("swift sdk configure", 1, "")
            .respond("xtool sdk install", 0, "");
        let root = PathBuf::from("/forge");
        let dir = tempfile::tempdir().unwrap();
        let xip = dir.path().join("Xcode_26.xip");
        std::fs::write(&xip, b"").unwrap();
        let env = |_: &str| None;
        let ask = |_: &str| true;
        let steps = run_steps(
            &host(&mock, &root, &env, &ask),
            &Options {
                xip: Some(xip),
                ..opts(true)
            },
        );
        match outcome(&steps, "Darwin SDK") {
            Outcome::Todo(t) => assert!(t.contains("can't find the iOS SDK"), "{t}"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn xtool_that_is_still_not_the_pinned_version_after_install_needs_the_person() {
        let mock = ready_machine();
        mock.respond("xtool --version", 0, "xtool 1.0.0\n").respond(
            "bash /forge/scripts/install-xtool.sh",
            0,
            "",
        );
        let root = PathBuf::from("/forge");
        let env = |_: &str| None;
        let ask = |_: &str| true;
        let steps = run_steps(&host(&mock, &root, &env, &ask), &opts(true));
        match outcome(&steps, "xtool") {
            Outcome::Todo(t) => assert!(t.contains("1.0.0") && t.contains("which -a xtool"), "{t}"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn setup_fails_when_doctor_disagrees_with_the_steps() {
        let all_ready = [Step {
            name: "xtool",
            outcome: Outcome::Ready("xtool 1".into()),
        }];
        assert!(verdict(&all_ready, true, &[]).is_ok());
        let err = verdict(&all_ready, false, &["install Swift 6".into()]).unwrap_err();
        assert!(
            err.contains("aren't ready") && err.contains("install Swift 6"),
            "{err}"
        );
        let todo = [Step {
            name: "Darwin SDK",
            outcome: Outcome::Todo("download Xcode".into()),
        }];
        assert!(verdict(&todo, true, &[])
            .unwrap_err()
            .contains("1 step still needs you"));
    }
    #[test]
    fn an_sdk_version_forge_cant_parse_is_never_compatible() {
        assert!(sdk_is_compatible("26.0"));
        assert!(sdk_is_compatible("27.1"));
        assert!(!sdk_is_compatible("18.0"));
        assert!(!sdk_is_compatible("iPhoneOS"));
        assert!(!sdk_is_compatible(""));

        let mock = ready_machine();
        mock.respond(
            "swift sdk configure",
            0,
            "sdkRootPath: /nonexistent/iPhoneOSbeta.sdk\n",
        );
        let root = PathBuf::from("/forge");
        let env = |_: &str| None;
        let ask = |_: &str| false;
        let steps = run_steps(&host(&mock, &root, &env, &ask), &opts(false));
        assert!(
            !matches!(outcome(&steps, "Darwin SDK"), Outcome::Ready(_)),
            "{steps:?}"
        );
    }
}
