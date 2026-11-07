//! Where the server may work: the root it was started in, and the apps
//! inside it. Every path a tool receives is resolved here, so nothing reaches
//! outside the root (tier 1 of the safety policy in docs/forge-mcp.md).

use std::path::{Component, Path, PathBuf};

/// What `forge.toml` says about an app, as far as the tools need it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct App {
    pub dir: PathBuf,
    pub name: String,
    pub bundle_id: String,
    /// The executable's name on the phone (`[ios] xtool_product`, else
    /// `AppHostXtool`, as in forge-cli), used to filter logs and crash reports.
    pub product: String,
    /// `[ios.signing] team_type`, when set.
    pub team_type: Option<String>,
}

/// Resolves `project` (relative to `root`, default the root itself) to an
/// app directory inside `root` that has a forge.toml.
pub fn app(root: &Path, project: Option<&str>) -> Result<App, String> {
    let dir = inside(root, project.unwrap_or("."))?;
    let toml_path = dir.join("forge.toml");
    let text = std::fs::read_to_string(&toml_path).map_err(|_| {
        format!(
            "{} is not a Forge app (no forge.toml). Pass `project`: the app's folder, relative to {}",
            dir.display(),
            root.display()
        )
    })?;
    let doc: toml::Table = text
        .parse()
        .map_err(|e| format!("{}: {e}", toml_path.display()))?;
    let get = |section: &str, key: &str| -> Option<String> {
        let mut table = &doc;
        for part in section.split('.') {
            table = table.get(part)?.as_table()?;
        }
        table.get(key)?.as_str().map(str::to_string)
    };
    let name = get("app", "name").ok_or("forge.toml has no [app] name")?;
    let bundle_id = get("app", "bundle_id").ok_or("forge.toml has no [app] bundle_id")?;
    let product = get("ios", "xtool_product").unwrap_or_else(|| "AppHostXtool".into());
    Ok(App {
        team_type: get("ios.signing", "team_type"),
        dir,
        name,
        bundle_id,
        product,
    })
}

/// `path` joined to `root`, lexically normalised, refused if it leaves
/// `root`. The target need not exist yet (a new app's folder).
pub fn inside(root: &Path, path: &str) -> Result<PathBuf, String> {
    let given = Path::new(path);
    let joined = if given.is_absolute() {
        given.to_path_buf()
    } else {
        root.join(given)
    };
    let mut out = PathBuf::new();
    for part in joined.components() {
        match part {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other),
        }
    }
    // Follow symlinks for the part that exists, so a link can't lead out.
    let real = existing_prefix_canonical(&out);
    if !real.starts_with(root) {
        return Err(format!(
            "{path} is outside {}: Forge MCP only works inside the folder it was started in (or --root)",
            root.display()
        ));
    }
    Ok(real)
}

fn existing_prefix_canonical(path: &Path) -> PathBuf {
    let mut missing = Vec::new();
    let mut cur = path.to_path_buf();
    loop {
        if let Ok(real) = cur.canonicalize() {
            let mut out = real;
            for part in missing.iter().rev() {
                out.push(part);
            }
            return out;
        }
        match (cur.file_name().map(|n| n.to_os_string()), cur.parent()) {
            (Some(name), Some(parent)) => {
                missing.push(name);
                cur = parent.to_path_buf();
            }
            _ => return path.to_path_buf(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_stay_inside_the_root() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        std::fs::create_dir(root.join("apps")).unwrap();
        assert_eq!(inside(&root, "apps/new").unwrap(), root.join("apps/new"));
        assert_eq!(inside(&root, ".").unwrap(), root);
        assert!(inside(&root, "../elsewhere").is_err());
        assert!(inside(&root, "apps/../../x").is_err());
        assert!(inside(&root, "/etc").is_err());
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink("/tmp", root.join("out")).unwrap();
            assert!(inside(&root, "out/x").is_err());
        }
    }

    #[test]
    fn app_reads_forge_toml() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        let dir = root.join("Tipster");
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(
            dir.join("forge.toml"),
            "[app]\nname = \"Tipster\"\nbundle_id = \"org.forgemobile.agentlab\"\n[ios]\nxtool_product = \"TipsterXtool\"\n[ios.signing]\nteam_type = \"paid\"\n",
        )
        .unwrap();
        let app = app(&root, Some("Tipster")).unwrap();
        assert_eq!(app.bundle_id, "org.forgemobile.agentlab");
        assert_eq!(app.product, "TipsterXtool");
        assert_eq!(app.team_type.as_deref(), Some("paid"));
        assert!(super::app(&root, None)
            .unwrap_err()
            .contains("not a Forge app"));
    }
}
