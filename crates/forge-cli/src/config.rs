use serde::Deserialize;

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct ForgeConfig {
    pub app: AppSection,
    pub rust: RustSection,
    pub build: BuildSection,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct AppSection {
    pub name: String,
    pub bundle_id: String,
    pub minimum_ios: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct RustSection {
    pub core_crate: String,
    pub ffi_crate: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct BuildSection {
    pub ios_device: bool,
    pub ios_simulator: bool,
}

impl ForgeConfig {
    pub fn parse(content: &str) -> Result<Self, String> {
        let cfg: ForgeConfig =
            toml::from_str(content).map_err(|e| format!("invalid forge.toml: {e}"))?;
        cfg.validate()?;
        Ok(cfg)
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
        if self.build.ios_simulator && !self.build.ios_device {
            return Err("ios_simulator requires ios_device in this release".into());
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
    fn valid_config() {
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
