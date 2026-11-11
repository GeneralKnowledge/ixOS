use serde::Deserialize;

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct ForgeConfig {
    pub app: AppSection,
    pub rust: RustSection,
    pub build: BuildSection,
    #[serde(default)]
    pub android: Option<AndroidSection>,
    /// iOS app settings Forge turns into Info.plist, App.entitlements and
    /// xtool.yml (see `app_config.rs` and docs/app-config.md).
    #[serde(default)]
    pub ios: Option<crate::app_config::IosSection>,
    /// How UI tests and `forge ui explore` run the app (docs/forge-explore.md).
    #[serde(default)]
    pub ui_test: Option<UiTestSection>,
    /// The app's Swift module names (docs/app-crate.md).
    #[serde(default)]
    pub swift: Option<SwiftSection>,
}

/// `[swift]`: the names of the app's Swift modules. Each defaults to Forge's
/// fixed name (`ForgeFFI`, `AppHost`). An app that another app hosts (Device
/// Lab's Apps screen) names its own, because one Swift build can hold only
/// one module of each name.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SwiftSection {
    /// The module of the app's UniFFI bindings (`Generated/ForgeFFI`).
    #[serde(default)]
    pub bindings_module: Option<String>,
    /// The module of the app's views and models (`Sources/AppHost`).
    #[serde(default)]
    pub app_module: Option<String>,
}

/// The Swift module of the UniFFI bindings when forge.toml names none.
pub const DEFAULT_BINDINGS_MODULE: &str = "ForgeFFI";
/// The Swift module of the app's views and models when forge.toml names none.
pub const DEFAULT_APP_MODULE: &str = "AppHost";

/// `[ui_test]`: what UI automation needs to know about the app.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct UiTestSection {
    /// Launch arguments that start the app on fresh, throwaway data instead
    /// of the person's own (Pocket Library: `["-ForgeUITestData"]`).
    /// `forge ui explore` refuses to change data without them.
    #[serde(default)]
    pub test_data_args: Vec<String>,
    /// Identifiers `forge ui explore --real-data` may tap besides
    /// navigation (rows, tabs, back, cancel), because the developer knows
    /// they change nothing: a button that opens a sheet, a filter. Never
    /// overrides the never-lists (system services, acting outside the
    /// phone).
    #[serde(default)]
    pub real_data_safe: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct AppSection {
    pub name: String,
    pub bundle_id: String,
    #[serde(default)]
    pub minimum_ios: Option<String>,
    /// Home-screen name; defaults to `name`.
    #[serde(default)]
    pub display_name: Option<String>,
    /// User-visible version (CFBundleShortVersionString); defaults to 1.0.0.
    #[serde(default)]
    pub version: Option<String>,
    /// Build number (CFBundleVersion); defaults to 1.
    #[serde(default)]
    pub build_number: Option<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct AndroidSection {
    pub min_sdk: u32,
    pub abis: Vec<String>,
    #[serde(default)]
    pub application_id: Option<String>,
}

/// Where the app's Rust code lives.
///
/// Every app owns a crate: `path = "rust"` (relative to the app, as `forge
/// init` creates it). Two host apps may share one crate (`counter-android`
/// uses `path = "../counter/rust"`). `core_crate` / `ffi_crate`, which named
/// the workspace's shared `forge-ffi` before it was removed, are still parsed
/// so the error can say what to write instead.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RustSection {
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub core_crate: Option<String>,
    #[serde(default)]
    pub ffi_crate: Option<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct BuildSection {
    #[serde(default)]
    pub ios_device: bool,
    #[serde(default)]
    pub ios_simulator: bool,
    #[serde(default)]
    pub android: bool,
}

impl ForgeConfig {
    pub fn parse(content: &str) -> Result<Self, String> {
        let cfg: ForgeConfig =
            toml::from_str(content).map_err(|e| format!("invalid forge.toml: {e}"))?;
        cfg.validate()?;
        Ok(cfg)
    }

    /// Only `[build] android = true` enables Android; an `[android]` section alone does not.
    pub fn android_enabled(&self) -> bool {
        self.build.android
    }

    /// The app's own Rust crate (`[rust] path`), relative to `app_dir`.
    pub fn rust_crate_dir(&self, app_dir: &std::path::Path) -> std::path::PathBuf {
        app_dir.join(self.rust.path.as_deref().unwrap_or("rust"))
    }

    /// The Swift module of the app's UniFFI bindings (`[swift] bindings_module`).
    pub fn bindings_module(&self) -> &str {
        self.swift
            .as_ref()
            .and_then(|s| s.bindings_module.as_deref())
            .unwrap_or(DEFAULT_BINDINGS_MODULE)
    }

    /// The Swift module of the app's views and models (`[swift] app_module`).
    pub fn app_module(&self) -> &str {
        self.swift
            .as_ref()
            .and_then(|s| s.app_module.as_deref())
            .unwrap_or(DEFAULT_APP_MODULE)
    }

    pub fn ios_enabled(&self) -> bool {
        self.build.ios_device || self.build.ios_simulator
    }

    pub fn android_application_id(&self) -> Result<String, String> {
        let section = self
            .android
            .as_ref()
            .ok_or("forge.toml missing [android] section")?;
        Ok(section
            .application_id
            .clone()
            .unwrap_or_else(|| self.app.bundle_id.clone()))
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.app.name.trim().is_empty() {
            return Err("app.name must not be empty".into());
        }
        for (key, name) in [
            ("bindings_module", self.bindings_module()),
            ("app_module", self.app_module()),
        ] {
            if !is_swift_module_name(name) {
                return Err(format!(
                    "[swift] {key} = \"{name}\" is not a Swift module name (letters, digits and _, not starting with a digit)"
                ));
            }
        }
        if self.bindings_module() == self.app_module() {
            return Err("[swift] bindings_module and app_module must differ".into());
        }
        if !is_valid_bundle_id(&self.app.bundle_id) {
            return Err(format!(
                "invalid bundle_id '{}': expected reverse-DNS (e.g. org.example.app)",
                self.app.bundle_id
            ));
        }

        if self.ios_enabled()
            && self
                .app
                .minimum_ios
                .as_ref()
                .is_none_or(|s| s.trim().is_empty())
        {
            return Err("minimum_ios is required when iOS builds are enabled".into());
        }

        if self.build.ios_simulator && !self.build.ios_device {
            return Err("ios_simulator requires ios_device in this release".into());
        }

        if self.android_enabled() {
            let section = self
                .android
                .as_ref()
                .ok_or("[android] section is required when Android builds are enabled")?;
            if section.min_sdk == 0 {
                return Err("android.min_sdk must be greater than zero".into());
            }
            if section.abis.is_empty() {
                return Err("android.abis must list at least one ABI".into());
            }
            for abi in &section.abis {
                if !is_valid_android_abi(abi) {
                    return Err(format!("unsupported android ABI '{abi}'"));
                }
            }
            let app_id = section
                .application_id
                .as_deref()
                .unwrap_or(self.app.bundle_id.as_str());
            if !is_valid_android_application_id(app_id) {
                return Err(format!(
                    "invalid android application_id '{app_id}': each segment must start with a letter and use only letters, digits and underscores (no hyphens)"
                ));
            }
        }

        crate::app_config::validate(self)?;

        if !self.ios_enabled() && !self.android_enabled() {
            return Err("enable at least one platform in [build] (ios_* or android)".into());
        }

        if self.rust.core_crate.is_some() || self.rust.ffi_crate.is_some() {
            return Err("[rust] core_crate / ffi_crate named the shared forge-ffi crate, which Forge no longer has: give the app its own crate and write [rust] path = \"rust\" (see docs/app-crate.md)".into());
        }
        match &self.rust.path {
            Some(path) => {
                if path.trim().is_empty() || std::path::Path::new(path).is_absolute() {
                    return Err(format!(
                        "[rust] path = \"{path}\" must be relative to the app, such as \"rust\""
                    ));
                }
            }
            None => return Err(
                "[rust] must set path = \"rust\" (the app's own crate, as forge init creates it)"
                    .into(),
            ),
        }
        Ok(())
    }
}

pub fn is_valid_android_abi(abi: &str) -> bool {
    matches!(abi, "arm64-v8a" | "x86_64" | "armeabi-v7a")
}

pub fn is_valid_bundle_id(id: &str) -> bool {
    let parts: Vec<&str> = id.split('.').collect();
    if parts.len() < 2 {
        return false;
    }
    parts.iter().all(|part| {
        !part.is_empty()
            && part
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            && part.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
    })
}

/// Android application IDs: two or more segments, each starting with a letter,
/// with only ASCII letters, digits and underscores.
pub fn is_valid_android_application_id(id: &str) -> bool {
    let parts: Vec<&str> = id.split('.').collect();
    parts.len() >= 2
        && parts.iter().all(|part| {
            part.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
                && part.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        })
}

/// Words that cannot be a segment of a Kotlin source package (Java reserved
/// words and literals, plus Kotlin hard keywords).
const RESERVED_PACKAGE_WORDS: &[&str] = &[
    "abstract",
    "as",
    "assert",
    "boolean",
    "break",
    "byte",
    "case",
    "catch",
    "char",
    "class",
    "const",
    "continue",
    "default",
    "do",
    "double",
    "else",
    "enum",
    "extends",
    "false",
    "final",
    "finally",
    "float",
    "for",
    "fun",
    "goto",
    "if",
    "implements",
    "import",
    "in",
    "instanceof",
    "int",
    "interface",
    "is",
    "long",
    "native",
    "new",
    "null",
    "object",
    "package",
    "private",
    "protected",
    "public",
    "return",
    "short",
    "static",
    "strictfp",
    "super",
    "switch",
    "synchronized",
    "this",
    "throw",
    "throws",
    "transient",
    "true",
    "try",
    "typealias",
    "typeof",
    "val",
    "var",
    "void",
    "volatile",
    "when",
    "while",
];

/// A bundle ID that `forge init --platform android` can turn into the Kotlin package.
pub fn is_valid_kotlin_package(id: &str) -> bool {
    is_valid_android_application_id(id)
        && id
            .split('.')
            .all(|part| !RESERVED_PACKAGE_WORDS.contains(&part))
}

/// A name SwiftPM and Swift accept for a module.
pub fn is_swift_module_name(name: &str) -> bool {
    !name.is_empty()
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        && name
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
}

pub fn is_valid_project_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        && name.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_ios_config() {
        let cfg = ForgeConfig::parse(
            r#"
[app]
name = "Counter"
bundle_id = "org.example.counter"
minimum_ios = "17.0"
[rust]
path = "rust"
[build]
ios_device = true
ios_simulator = false
"#,
        )
        .unwrap();
        assert_eq!(cfg.app.name, "Counter");
        assert_eq!(cfg.bindings_module(), "ForgeFFI");
        assert_eq!(cfg.app_module(), "AppHost");
    }

    #[test]
    fn swift_module_names() {
        let base = "[app]\nname = \"Chat\"\nbundle_id = \"org.example.chat\"\nminimum_ios = \"17.0\"\n[rust]\npath = \"rust\"\n[build]\nios_device = true\n";
        let cfg = ForgeConfig::parse(&format!(
            "{base}[swift]\nbindings_module = \"ChatFFI\"\napp_module = \"ChatApp\"\n"
        ))
        .unwrap();
        assert_eq!(
            (cfg.bindings_module(), cfg.app_module()),
            ("ChatFFI", "ChatApp")
        );
        for bad in [
            "bindings_module = \"Chat-FFI\"",
            "app_module = \"1Chat\"",
            "app_module = \"ForgeFFI\"",
            "module = \"Chat\"",
        ] {
            assert!(
                ForgeConfig::parse(&format!("{base}[swift]\n{bad}\n")).is_err(),
                "{bad}"
            );
        }
    }

    #[test]
    fn valid_android_only_config() {
        let cfg = ForgeConfig::parse(
            r#"
[app]
name = "Counter"
bundle_id = "org.example.counter"

[android]
min_sdk = 26
abis = ["arm64-v8a", "x86_64"]

[rust]
path = "rust"

[build]
android = true
"#,
        )
        .unwrap();
        assert!(cfg.android_enabled());
        assert_eq!(cfg.android_application_id().unwrap(), "org.example.counter");
    }

    #[test]
    fn rejects_android_without_section() {
        let err = ForgeConfig::parse(
            r#"
[app]
name = "Counter"
bundle_id = "org.example.counter"
[rust]
path = "rust"
[build]
android = true
"#,
        )
        .unwrap_err();
        assert!(err.contains("[android]"));
    }

    #[test]
    fn android_section_alone_does_not_enable_android() {
        let cfg = ForgeConfig::parse(
            r#"
[app]
name = "Counter"
bundle_id = "org.example.counter"
minimum_ios = "17.0"

[android]
min_sdk = 26
abis = ["arm64-v8a"]

[rust]
path = "rust"

[build]
ios_device = true
android = false
"#,
        )
        .unwrap();
        assert!(!cfg.android_enabled());
        assert!(cfg.ios_enabled());
    }

    #[test]
    fn rejects_hyphenated_android_application_id() {
        let err = ForgeConfig::parse(
            r#"
[app]
name = "Counter"
bundle_id = "org.my-app.counter"

[android]
min_sdk = 26
abis = ["arm64-v8a"]

[rust]
path = "rust"

[build]
android = true
"#,
        )
        .unwrap_err();
        assert!(err.contains("application_id"), "{err}");
    }

    #[test]
    fn kotlin_package_rules() {
        assert!(is_valid_kotlin_package("org.example.my_app"));
        assert!(!is_valid_kotlin_package("org.my-app.demo"));
        assert!(!is_valid_kotlin_package("org.example.fun"));
        assert!(!is_valid_kotlin_package("org.example.1app"));
        assert!(!is_valid_kotlin_package("single"));
        // iOS bundle IDs may still use hyphens.
        assert!(is_valid_bundle_id("org.my-app.demo"));
    }

    #[test]
    fn rejects_unsupported_ffi_crate() {
        let err = ForgeConfig::parse(
            r#"
[app]
name = "Counter"
bundle_id = "org.example.counter"
minimum_ios = "17.0"
[rust]
core_crate = "forge-core"
ffi_crate = "my-ffi"
[build]
ios_device = true
ios_simulator = false
"#,
        )
        .unwrap_err();
        assert!(err.contains("ffi_crate"), "{err}");
        assert!(err.contains("path = \"rust\""), "{err}");
    }

    /// Every example's forge.toml must pass the CLI's own validation, so
    /// `forge build --project examples/<name>` works for each of them.
    #[test]
    fn app_owned_crate_path() {
        let toml = |rust: &str| {
            format!(
                "[app]\nname = \"A\"\nbundle_id = \"org.example.a\"\nminimum_ios = \"17.0\"\n[rust]\n{rust}\n[build]\nios_device = true\n"
            )
        };
        let cfg = ForgeConfig::parse(&toml("path = \"rust\"")).unwrap();
        assert_eq!(
            cfg.rust_crate_dir(std::path::Path::new("/apps/A")),
            std::path::PathBuf::from("/apps/A/rust")
        );
        // Two host apps may share one crate.
        let shared = ForgeConfig::parse(&toml("path = \"../counter/rust\"")).unwrap();
        assert_eq!(
            shared.rust_crate_dir(std::path::Path::new("/apps/A")),
            std::path::PathBuf::from("/apps/A/../counter/rust")
        );
        for bad in ["path = \"/abs\"", "path = \"\""] {
            assert!(ForgeConfig::parse(&toml(bad)).is_err(), "{bad}");
        }
        let both = "path = \"rust\"\ncore_crate = \"forge-core\"\nffi_crate = \"forge-ffi\"";
        assert!(ForgeConfig::parse(&toml(both)).is_err());
        let err = ForgeConfig::parse(&toml("")).unwrap_err();
        assert!(err.contains("path = \"rust\""), "{err}");
        assert!(ForgeConfig::parse(&toml("crate = \"rust\"")).is_err());
    }

    #[test]
    fn every_example_config_is_valid() {
        let examples = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
        let mut checked = 0;
        for entry in std::fs::read_dir(&examples).unwrap() {
            let path = entry.unwrap().path();
            // Tests that run `forge init` stage a template copy in examples/.<name>.forge-init.
            if path
                .file_name()
                .is_some_and(|name| name.as_encoded_bytes().starts_with(b"."))
            {
                continue;
            }
            let toml = path.join("forge.toml");
            if !toml.is_file() {
                continue;
            }
            let content = std::fs::read_to_string(&toml).unwrap();
            if let Err(err) = ForgeConfig::parse(&content) {
                panic!("{}: {err}", toml.display());
            }
            checked += 1;
        }
        assert!(checked >= 5, "expected every example, found {checked}");
    }
}
