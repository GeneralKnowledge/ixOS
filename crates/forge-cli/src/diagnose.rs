//! Turns the output of a failed build or install into "what went wrong and how
//! to fix it".
//!
//! The iOS pipeline has three layers that fail in different ways: the Rust
//! static library (Cargo, the iOS target, the Darwin SDK), the Swift app (xtool
//! and SwiftPM against the bindings), and signing and installing on the device
//! (Apple ID, provisioning, the iPhone itself). Their raw errors rarely say
//! what to do next, so each known failure is matched here by a fragment of its
//! output and mapped to a fix. Unknown failures get no hint; the raw output is
//! still on the terminal above the error.

/// One recognised failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hint {
    /// Which part of the pipeline failed.
    pub stage: Stage,
    /// What went wrong, in a sentence.
    pub problem: &'static str,
    /// What to do about it.
    pub fix: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Rust,
    Swift,
    Signing,
    Device,
}

impl Stage {
    pub fn label(self) -> &'static str {
        match self {
            Stage::Rust => "Rust library",
            Stage::Swift => "Swift build",
            Stage::Signing => "Signing and provisioning",
            Stage::Device => "iPhone",
        }
    }

    /// The name in `--json` results (`stage`): rust, swift, signing, device.
    pub fn key(self) -> &'static str {
        match self {
            Stage::Rust => "rust",
            Stage::Swift => "swift",
            Stage::Signing => "signing",
            Stage::Device => "device",
        }
    }
}

struct Rule {
    /// Matches when the output contains any of these (case-insensitive).
    any: &'static [&'static str],
    hint: Hint,
}

const fn rule(
    any: &'static [&'static str],
    stage: Stage,
    problem: &'static str,
    fix: &'static str,
) -> Rule {
    Rule {
        any,
        hint: Hint {
            stage,
            problem,
            fix,
        },
    }
}

/// Most specific first; [`diagnose`] returns every rule that matches, in this order.
const RULES: &[Rule] = &[
    // --- Rust library -----------------------------------------------------
    rule(
        &["target may not be installed", "can't find crate for `core`", "can't find crate for `std`"],
        Stage::Rust,
        "The Rust standard library for iOS (aarch64-apple-ios) is not installed.",
        "rustup target add aarch64-apple-ios",
    ),
    rule(
        &["sdkroot is not set", "darwin sdk not found", "could not find the darwin sdk"],
        Stage::Rust,
        "The Darwin SDK that xtool installs is missing, so the Rust library cannot be linked for iOS.",
        "Run `xtool setup` and give it Xcode.xip from Xcode 26 (docs/linux-setup.md). `swift sdk list` should then list darwin.",
    ),
    rule(
        &["error: could not compile"],
        Stage::Rust,
        "Your Rust code does not compile.",
        "Fix the Rust errors above, then check with `cargo check --manifest-path rust/Cargo.toml` (fast, no iOS build) before running forge again.",
    ),
    // --- Swift build ------------------------------------------------------
    rule(
        &["swift_coroframealloc"],
        Stage::Swift,
        "The Darwin SDK is too old for this Swift compiler (Swift 6.2 needs the iOS 26 SDK; Xcode 16's iOS 18 SDK lacks swift_coroFrameAlloc).",
        "Run `xtool setup` again with Xcode.xip from Xcode 26 (docs/troubleshooting.md).",
    ),
    rule(
        &["darwin sdk is out of date", "darwin sdk is incompatible"],
        Stage::Swift,
        "xtool's Darwin SDK was built by an older xtool.",
        "Reinstall it: `xtool sdk install` with your Xcode.xip (Xcode 26).",
    ),
    rule(
        &["no such module 'forgeffi'", "no such module 'forge_ffiffi'", "missing required module 'forge_ffiffi'"],
        Stage::Swift,
        "The Swift bindings for your Rust crate are missing.",
        "Regenerate them with `forge bindings generate` from the app directory (forge build ios also does this).",
    ),
    rule(
        &["undefined symbol: _uniffi_", "undefined symbol: uniffi_", "undefined symbols for architecture arm64"],
        Stage::Swift,
        "The Swift bindings and the Rust static library do not match (one is older than the other) or the library was not built for iOS.",
        "Delete the app's build/ios-device folder and run the forge command again so both are rebuilt together.",
    ),
    rule(
        &["libforge_ffi.a: no such file", "library not found for -lforge_ffi", "unable to find library -lforge_ffi"],
        Stage::Swift,
        "The Rust static library has not been built for iOS.",
        "Run `forge build ios --package-only` from the app directory, or the full `forge build ios`, which builds it first.",
    ),
    rule(
        &["could not find package.swift"],
        Stage::Swift,
        "xtool was run outside the app directory.",
        "Run forge from the app directory (the one with forge.toml and Package.swift), or pass --project <dir>.",
    ),
    // --- Signing and provisioning -----------------------------------------
    rule(
        &["requires logging in with `xtool auth`", "logged out", "not logged in", "no saved auth token"],
        Stage::Signing,
        "xtool is not signed in to an Apple account, so it cannot sign the app.",
        "Run `xtool auth login` (Apple ID with password, or an App Store Connect API key for paid accounts). Check with `xtool auth status`.",
    ),
    rule(
        &[
            "maximum number of apps",
            "maxappsreached",
            "maximum number of installed apps",
            "using a free developer profile",
        ],
        Stage::Signing,
        "A free Apple account can have at most 3 sideloaded apps on a device at once.",
        "Delete one of your other development apps from the iPhone (long-press its icon, Remove App), then run again.",
    ),
    rule(
        &["app id limit", "maximum app id", "you may create up to 10 app ids", "more than 10 app ids"],
        Stage::Signing,
        "A free Apple account can register only 10 App IDs (bundle identifiers) every 7 days.",
        "Reuse a bundle identifier you already registered (bundle_id in forge.toml), or wait until the 7-day window passes. A paid Developer Program account has no such limit.",
    ),
    rule(
        &["no development teams found"],
        Stage::Signing,
        "xtool could not find a development team for this Apple account.",
        "Log in again with `xtool auth login` and pick a team. A new Apple ID may first need to accept the developer agreement at developer.apple.com.",
    ),
    rule(
        &["provisioning profile", "no profiles for", "certificate has been revoked", "too many certificates", "maximum number of certificates"],
        Stage::Signing,
        "Apple refused the signing certificate or provisioning profile for this app.",
        "Run `xtool auth logout` then `xtool auth login` to get fresh credentials. With a free account, revoke old development certificates at developer.apple.com (Certificates, IDs & Profiles) if you have hit the limit.",
    ),
    rule(
        &["incorrect password", "invalid username or password", "authentication failed", "two-factor"],
        Stage::Signing,
        "Signing in to Apple failed.",
        "Check the Apple ID and password with `xtool auth login`. If two-factor codes stop arriving, `xtool auth logout --reset-2fa` and log in again.",
    ),
    // --- Device -----------------------------------------------------------
    rule(
        &["developer mode"],
        Stage::Device,
        "Developer Mode is off on the iPhone (required since iOS 16 to run apps you build).",
        "On the iPhone: Settings > Privacy & Security > Developer Mode, turn it on, restart, and confirm. Then run again.",
    ),
    rule(
        &["has not been explicitly trusted", "untrusted developer", "invalid code signature, inadequate entitlements"],
        Stage::Device,
        "The app installed, but the iPhone does not trust your developer profile yet, so it will not launch.",
        "On the iPhone: Settings > General > VPN & Device Management, tap your Apple ID, then Trust. Open the app from the home screen.",
    ),
    rule(
        &["device is locked", "devicelocked", "passwordprotected", "password protected"],
        Stage::Device,
        "The iPhone is locked.",
        "Unlock the iPhone and keep it unlocked while forge installs, then run again.",
    ),
    rule(
        &["pairingdialog", "invalidhostid", "could not connect to lockdownd", "not paired", "trust this computer"],
        Stage::Device,
        "The iPhone and this computer are not paired.",
        "Unplug and replug the iPhone, unlock it and tap Trust when it asks. If it never asks: `idevicepair pair` (libimobiledevice-utils), then run again.",
    ),
    rule(
        &["the iphone was disconnected"],
        Stage::Device,
        "The iPhone was unplugged or dropped off USB during the run.",
        "Plug the iPhone back in, unlock it, check `forge devices` lists it, then run again. What was collected before is kept in the run's folder.",
    ),
    rule(
        &["usbmuxd", "usbmux", "resource temporarily unavailable"],
        Stage::Device,
        "usbmuxd, the service that talks to iPhones over USB, is not running or stopped responding.",
        "Run `sudo systemctl restart usbmuxd`, replug and unlock the iPhone, then check with `forge devices`.",
    ),
    rule(
        &["no devices", "device not found", "could not find device", "waiting for devices"],
        Stage::Device,
        "No iPhone is connected.",
        "Connect the iPhone by USB, unlock it, tap Trust if asked, and check that `forge devices` lists it.",
    ),
    rule(
        &["no space left on device"],
        Stage::Device,
        "A disk is full (this computer's or the iPhone's).",
        "Free some space (the app's rust/target and build folders can be deleted safely), then run again.",
    ),
];

/// Every known failure whose fingerprint is in `output`, most specific first,
/// without repeating a fix.
pub fn diagnose(output: &str) -> Vec<Hint> {
    let lower = output.to_lowercase();
    let mut hints: Vec<Hint> = Vec::new();
    for rule in RULES {
        if rule.any.iter().any(|needle| lower.contains(needle))
            && !hints.iter().any(|h| h.fix == rule.hint.fix)
        {
            hints.push(rule.hint.clone());
        }
    }
    hints
}

/// The error message for a failed step: what failed, then each diagnosis.
pub fn explain(step: &str, status: impl std::fmt::Display, output: &str) -> String {
    let hints = diagnose(output);
    crate::report::record_failure(
        step,
        hints
            .iter()
            .map(crate::report::JsonHint::from_hint)
            .collect(),
        output,
    );
    let mut msg = format!("`{step}` failed ({status})");
    if hints.is_empty() {
        msg.push_str(
            "\n\nForge did not recognise this failure. The output above has the details; `forge doctor` checks the toolchain, and docs/troubleshooting.md lists known problems.",
        );
        return msg;
    }
    msg.push_str("\n\nWhat went wrong:");
    for h in &hints {
        msg.push_str(&format!(
            "\n  [{}] {}\n    Fix: {}",
            h.stage.label(),
            h.problem,
            h.fix
        ));
    }
    msg
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stages(output: &str) -> Vec<Stage> {
        diagnose(output).iter().map(|h| h.stage).collect()
    }

    #[test]
    fn recognises_each_layer() {
        assert_eq!(
            stages("error[E0463]: can't find crate for `core`\n  = note: the `aarch64-apple-ios` target may not be installed"),
            [Stage::Rust]
        );
        assert_eq!(
            stages("ld.lld: error: undefined symbol: swift_coroFrameAlloc"),
            [Stage::Swift]
        );
        assert_eq!(
            stages("error: `build --sign` requires logging in with `xtool auth`"),
            [Stage::Signing]
        );
        assert_eq!(
            stages(
                "Error: The maximum number of apps for free development profiles has been reached."
            ),
            [Stage::Signing]
        );
        // iOS 26 wording, seen through xtool's applicationVerificationFailed.
        assert_eq!(
            stages(
                "This device has reached the maximum number of installed apps using a free developer profile."
            ),
            [Stage::Signing]
        );
        assert_eq!(
            stages("Error: To use iPhone for development, enable Developer Mode in Settings"),
            [Stage::Device]
        );
        assert_eq!(
            stages("Unable to launch org.example.app because it has an invalid code signature, inadequate entitlements or its profile has not been explicitly trusted by the user."),
            [Stage::Device]
        );
    }

    #[test]
    fn rust_compile_error_points_at_cargo_check() {
        let hints = diagnose("error[E0425]: cannot find value `x`\nerror: could not compile `my-app-core` (lib) due to 1 previous error");
        assert_eq!(hints.len(), 1);
        assert!(hints[0]
            .fix
            .contains("cargo check --manifest-path rust/Cargo.toml"));
    }

    #[test]
    fn stale_bindings_suggest_a_clean_rebuild() {
        let hints =
            diagnose("ld.lld: error: undefined symbol: uniffi_forge_ffi_checksum_method_notes_add");
        assert!(hints[0].fix.contains("build/ios-device"), "{hints:?}");
    }

    #[test]
    fn unknown_output_has_no_hint_but_says_where_to_look() {
        assert!(diagnose("something odd happened").is_empty());
        let msg = explain("xtool dev run", "exit status: 1", "something odd happened");
        assert!(msg.contains("forge doctor"), "{msg}");
        assert!(msg.contains("docs/troubleshooting.md"), "{msg}");
    }

    #[test]
    fn explain_lists_every_hint_with_its_fix() {
        let msg = explain(
            "xtool dev run",
            "exit status: 1",
            "usbmuxd: connection refused\nDeveloper Mode is disabled",
        );
        assert!(
            msg.starts_with("`xtool dev run` failed (exit status: 1)"),
            "{msg}"
        );
        assert!(msg.contains("[iPhone] Developer Mode is off"), "{msg}");
        assert!(msg.contains("systemctl restart usbmuxd"), "{msg}");
    }
}
