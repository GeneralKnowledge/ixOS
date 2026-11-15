//! `forge verify --accept <ID> --note "…"` records a person's acceptance of
//! one requirement for the app's files as they are now, as its own bundle
//! (`kind = "human"`). It asks the person to type the id back, and it is
//! refused unless stdin is a terminal and outside an MCP server
//! (`FORGE_MCP_CHILD`), so no agent can record it.

use std::fs;
use std::path::Path;

use serde_json::json;

use crate::evidence;
use crate::runner::CommandRunner;

fn git(runner: &dyn CommandRunner, app: &Path, args: &[&str]) -> Option<String> {
    evidence::git_line(runner, app, args, None)
}

/// Why `--accept` can't run here, if it can't: only a person at a terminal
/// records an acceptance.
pub fn person_present(stdin_is_terminal: bool, under_mcp: bool) -> Result<(), String> {
    if under_mcp {
        return Err("forge verify --accept is refused inside forge mcp: only a person records an acceptance, from their own terminal".into());
    }
    if !stdin_is_terminal {
        return Err("forge verify --accept needs a person at a terminal (stdin is not one): an agent or a script can't record an acceptance".into());
    }
    Ok(())
}

/// `forge verify --accept`.
pub fn accept(
    runner: &dyn CommandRunner,
    app: &Path,
    id: &str,
    note: &str,
    confirm: &mut dyn std::io::BufRead,
    out: &mut dyn std::io::Write,
) -> Result<(), String> {
    use std::io::IsTerminal;
    person_present(
        std::io::stdin().is_terminal(),
        std::env::var_os("FORGE_MCP_CHILD").is_some(),
    )?;
    accept_confirmed(runner, app, id, note, confirm, out)
}

fn accept_confirmed(
    runner: &dyn CommandRunner,
    app: &Path,
    id: &str,
    note: &str,
    confirm: &mut dyn std::io::BufRead,
    out: &mut dyn std::io::Write,
) -> Result<(), String> {
    let reqs = evidence::load_requirements(app)?;
    let Some(req) = reqs.iter().find(|r| r.id == id) else {
        return Err(format!(
            "{id} is not in {} (it has: {})",
            app.join(evidence::REQUIREMENTS_FILE).display(),
            reqs.iter()
                .map(|r| r.id.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    };
    if note.trim().is_empty() {
        return Err("--note is required: say what you looked at on the phone".into());
    }
    let build = evidence::provenance(runner, app);
    let w = |out: &mut dyn std::io::Write, s: String| {
        out.write_all(s.as_bytes()).map_err(|e| e.to_string())
    };
    w(
        out,
        format!(
            "Accept {id} \"{}\" for the app's files as they are now (commit {}{})?\nType {id} to confirm: ",
            req.statement,
            build["commit"].as_str().unwrap_or("unknown"),
            if build["dirty"] == json!(true) { ", with uncommitted changes" } else { "" }
        ),
    )?;
    out.flush().map_err(|e| e.to_string())?;
    let mut line = String::new();
    confirm.read_line(&mut line).map_err(|e| e.to_string())?;
    if line.trim() != id {
        return Err("not confirmed: nothing recorded".into());
    }
    let by = git(runner, app, &["config", "user.name"])
        .filter(|s| !s.is_empty())
        .or_else(|| std::env::var("USER").ok())
        .unwrap_or_else(|| "unknown".into());
    let stamp = crate::report::timestamp();
    let doc = json!({
        "schema": evidence::SCHEMA,
        "id": stamp,
        "kind": "human",
        "command": "verify --accept",
        "ok": true,
        "build": build,
        "requirements": [{
            "id": req.id,
            "statement": req.statement,
            "person": {"verdict": "accepted", "by": by, "note": note},
        }],
    });
    let dir = app.join(".forge/runs").join(format!("{stamp}-accept"));
    fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    fs::write(
        dir.join("evidence.json"),
        serde_json::to_string_pretty(&doc).unwrap_or_default() + "\n",
    )
    .map_err(|e| format!("evidence.json: {e}"))?;
    crate::report::set_data(json!({"accepted": id, "by": by, "bundle": dir.join("evidence.json")}));
    w(
        out,
        format!(
            "\nRecorded: {id} accepted by {by}. {}\n",
            dir.join("evidence.json").display()
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::verify;

    #[test]
    fn only_a_person_at_a_terminal_can_accept() {
        assert!(person_present(false, false)
            .unwrap_err()
            .contains("stdin is not one"));
        assert!(person_present(true, true)
            .unwrap_err()
            .contains("inside forge mcp"));
        assert!(person_present(true, false).is_ok());
    }

    #[test]
    fn an_acceptance_needs_the_id_typed_back_and_names_the_files() {
        let tmp = tempfile::tempdir().unwrap();
        let app = tmp.path();
        fs::write(
            app.join(evidence::REQUIREMENTS_FILE),
            "[[requirement]]\nid = \"GD-1\"\nstatement = \"Record an outcome\"\ndevice = [\"uitests/journey.toml\"]\n",
        )
        .unwrap();
        let runner = crate::runner::MockRunner::default();
        runner.respond("git", 0, "Reuben\n");
        let mut out = Vec::new();
        let refused = accept_confirmed(
            &runner,
            app,
            "GD-1",
            "Looked on Cati",
            &mut "no\n".as_bytes(),
            &mut out,
        );
        assert_eq!(refused.unwrap_err(), "not confirmed: nothing recorded");
        assert!(!app.join(".forge/runs").exists());
        assert!(accept_confirmed(
            &runner,
            app,
            "GD-9",
            "x",
            &mut "GD-9\n".as_bytes(),
            &mut out
        )
        .is_err());
        assert!(accept_confirmed(
            &runner,
            app,
            "GD-1",
            " ",
            &mut "GD-1\n".as_bytes(),
            &mut out
        )
        .is_err());
        accept_confirmed(
            &runner,
            app,
            "GD-1",
            "Looked on Cati",
            &mut "GD-1\n".as_bytes(),
            &mut out,
        )
        .unwrap();
        let bundles = verify::bundles(app);
        assert_eq!(bundles.len(), 1);
        assert_eq!(bundles[0]["kind"], "human");
        assert_eq!(
            bundles[0]["requirements"][0]["person"]["verdict"],
            "accepted"
        );
        assert_eq!(
            bundles[0]["requirements"][0]["person"]["note"],
            "Looked on Cati"
        );
        assert_eq!(bundles[0]["requirements"][0]["person"]["by"], "Reuben");
        assert!(String::from_utf8(out)
            .unwrap()
            .contains("Type GD-1 to confirm"));
    }
}
