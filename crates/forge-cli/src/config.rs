use serde::Deserialize;

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct ForgeConfig {
    pub app: AppSection,
    pub rust: RustSection,
    pub build: BuildSection,
    #[serde(default)]
    pub android: Option<AndroidSection>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct AppSection {
    pub name: String,
    pub bundle_id: String,
    #[serde(default)]
    pub minimum_ios: Option<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct AndroidSection {
    pub min_sdk: u32,
    pub abis: Vec<String>,
    #[serde(default)]
    pub application_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct RustSection {
    pub core_crate: String,
    pub ffi_crate: String,
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

    pub fn android_enabled(&self) -> bool {
        self.build.android || self.android.is_some()
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
            if !is_valid_bundle_id(app_id) {
                return Err(format!("invalid android application_id '{app_id}'"));
            }
        }

        if !self.ios_enabled() && !self.android_enabled() {
            return Err("enable at least one platform in [build] (ios_* or android)".into());
        }

        if self.rust.core_crate != "forge-core" {
            return Err(format!(
                "unsupported core_crate '{}': v0.1 only builds forge-core from the workspace root",
                self.rust.core_crate
            ));
        }
        if self.rust.ffi_crate != "forge-ffi" {
            return Err(format!(
                "unsupported ffi_crate '{}': v0.1 only builds forge-ffi from the workspace root",
                self.rust.ffi_crate
            ));
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
core_crate = "forge-core"
ffi_crate = "forge-ffi"
[build]
ios_device = true
ios_simulator = false
"#,
        )
        .unwrap();
        assert_eq!(cfg.app.name, "Counter");
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
core_crate = "forge-core"
ffi_crate = "forge-ffi"

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
core_crate = "forge-core"
ffi_crate = "forge-ffi"
[build]
android = true
"#,
        )
        .unwrap_err();
        assert!(err.contains("[android]"));
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
        assert!(err.contains("ffi_crate"));
    }
}
