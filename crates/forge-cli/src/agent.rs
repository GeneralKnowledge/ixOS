//! `forge agent setup`: adds the optional files that configure an AI coding
//! agent for an app. Nothing in Forge reads them to build, test or run the
//! app, and `forge init` doesn't write them unless asked (`--agent`).
//!
//! - `AGENTS.md`: the app's rules that are easy to get wrong, and the
//!   commands (templates/agent/AGENTS.md). Short on purpose: it points at
//!   Forge's docs instead of repeating them.
//! - `CLAUDE.md`: `@AGENTS.md`, for clients that read that name.
//! - `.mcp.json`: starts `forge mcp`, an optional layer over the same
//!   commands. `--apple-docs` adds the hosted sosumi server, which serves
//!   developer.apple.com pages as text (a third-party service).
//!
//! An existing file is never overwritten: the person may have added notes.

use std::path::Path;

use crate::config::ForgeConfig;

const TEMPLATE: &str = "templates/agent";

/// sosumi.ai: Apple's developer documentation as text, over MCP.
const APPLE_DOCS: &str = r#"    "sosumi": {
      "type": "http",
      "url": "https://sosumi.ai/mcp"
    }"#;

/// What [`setup`] did with each file.
#[derive(Debug, PartialEq, Eq)]
pub struct Outcome {
    pub written: Vec<&'static str>,
    pub kept: Vec<&'static str>,
}

/// Writes the agent files into `app` from the checkout at `root`.
pub fn setup(root: &Path, app: &Path, apple_docs: bool) -> Result<Outcome, String> {
    let forge_toml = app.join("forge.toml");
    let text = std::fs::read_to_string(&forge_toml)
        .map_err(|e| format!("{}: {e} (pass --project <app>)", forge_toml.display()))?;
    let cfg = ForgeConfig::parse(&text)?;
    let template = root.join(TEMPLATE);
    let read = |name: &str| {
        std::fs::read_to_string(template.join(name))
            .map_err(|e| format!("{}: {e}", template.join(name).display()))
    };
    let agents = read("AGENTS.md")?
        .replace("FORGE_APP_NAME", &cfg.app.name)
        .replace(
            "FORGE_XTOOL_PRODUCT",
            &crate::app_config::xtool_product(&cfg),
        );
    let mut mcp = read("mcp.json")?;
    if apple_docs {
        mcp = mcp.replacen(
            "    }\n  }\n}",
            &format!("    }},\n{APPLE_DOCS}\n  }}\n}}"),
            1,
        );
    }
    let files: [(&'static str, String); 3] = [
        ("AGENTS.md", agents),
        ("CLAUDE.md", read("CLAUDE.md")?),
        (".mcp.json", mcp),
    ];
    let mut outcome = Outcome {
        written: Vec::new(),
        kept: Vec::new(),
    };
    for (name, content) in files {
        let path = app.join(name);
        if path.exists() {
            outcome.kept.push(name);
            continue;
        }
        std::fs::write(&path, content).map_err(|e| format!("{}: {e}", path.display()))?;
        outcome.written.push(name);
    }
    Ok(outcome)
}

/// `forge agent setup`, with what it did printed (or as `data`).
pub fn run(root: &Path, app: &Path, apple_docs: bool) -> Result<(), String> {
    let outcome = setup(root, app, apple_docs)?;
    crate::report::set_data(serde_json::json!({
        "written": outcome.written,
        "kept": outcome.kept,
    }));
    for name in &outcome.written {
        println!("wrote {name}");
    }
    for name in &outcome.kept {
        println!("kept {name} (it exists; delete it first to get the template's)");
    }
    println!(
        "These files are optional: nothing in Forge needs them to build, test or run the app. Commit them if your team uses agents."
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    fn app() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join("forge.toml"),
            "[app]\nname = \"Tipster\"\nbundle_id = \"org.example.tipster\"\nminimum_ios = \"18.0\"\n\n[rust]\npath = \"rust\"\n\n[build]\nios_device = true\n\n[ios]\nxtool_product = \"TipsterXtool\"\n",
        )
        .unwrap();
        tmp
    }

    #[test]
    fn writes_the_files_named_after_the_app_and_keeps_existing_ones() {
        let tmp = app();
        std::fs::write(tmp.path().join("CLAUDE.md"), "mine").unwrap();
        let out = setup(&root(), tmp.path(), false).unwrap();
        assert_eq!(out.written, vec!["AGENTS.md", ".mcp.json"]);
        assert_eq!(out.kept, vec!["CLAUDE.md"]);
        let agents = std::fs::read_to_string(tmp.path().join("AGENTS.md")).unwrap();
        assert!(agents.contains("Tipster is a native iOS app"), "{agents}");
        assert!(agents.contains("--process TipsterXtool"), "{agents}");
        assert!(!agents.contains("FORGE_"), "{agents}");
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("CLAUDE.md")).unwrap(),
            "mine"
        );
        let mcp: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(tmp.path().join(".mcp.json")).unwrap())
                .unwrap();
        assert_eq!(mcp["mcpServers"]["forge"]["args"][0], "mcp");
        // No hosted service unless asked for.
        assert!(mcp["mcpServers"].get("sosumi").is_none());
        // Running it again changes nothing.
        let again = setup(&root(), tmp.path(), false).unwrap();
        assert!(again.written.is_empty());
    }

    #[test]
    fn apple_docs_add_the_sosumi_server() {
        let tmp = app();
        setup(&root(), tmp.path(), true).unwrap();
        let mcp: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(tmp.path().join(".mcp.json")).unwrap())
                .unwrap();
        assert_eq!(mcp["mcpServers"]["sosumi"]["url"], "https://sosumi.ai/mcp");
        assert_eq!(mcp["mcpServers"]["forge"]["command"], "forge");
    }
}
