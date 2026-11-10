# Human first, agent optional: audit and changes (2026-11-09)

The question: can a developer who uses no AI agent build, test, debug and maintain an ixOS app with the documented tools? And is any agent-specific complexity in the framework unnecessary? The contract that came out of it is [human-workflow.md](../human-workflow.md), and the rule is in [architecture.md](../architecture.md#framework-principles) (rule 3) and the root `AGENTS.md`.

## Baseline (main at 02a05cc)

`forge mcp` was already mostly thin. Nearly every tool runs a `forge --json` child process and returns its result. What it implemented on its own, or what only worked through it:

| Finding | Where | Effect on a person without an agent |
|---|---|---|
| Opening the app on the phone had no command | `forge_launch_app` built a one-step TOML file in `tools.rs` | Write a UI test file by hand, or tap the icon |
| Project inspection existed only in MCP | `crates/forge-mcp/src/inspect.rs` | No `forge inspect` |
| Inspection labelled Swift files "view" or "model" from their imports, and its identifier scan duplicated `forge ui focus`'s, without its handling of interpolated identifiers | `inspect.rs` | Guesses that looked like facts |
| `forge verify --run` took builds, installs and failures only from the MCP journal; text-mode commands wrote no log or history | `run_record.rs`, `main.rs` | A run at the terminal had an empty "What ran" |
| Every `forge init` app had `AGENTS.md`, `CLAUDE.md` and a `.mcp.json` that also started a hosted documentation server | `templates/ios-app/` | Agent configuration in every app, and a third-party service in its MCP config |
| On real data, the explorer tapped any control that no word list flagged | `explore.rs` `gate` | An alert's "OK" after "Remove all?" counted as safe |

## Changed

1. **Real-data exploration is read-only by an allow list** (`explore.rs`). It taps rows, tabs, the back button, Back, Cancel, Close and Dismiss, and types only into search fields. Anything else is skipped with its reason, unless the developer lists it in forge.toml `[ui_test] real_data_safe`. The never-lists still win. Test-data mode is unchanged, and the policy for both modes is one table in [forge-explore.md](../forge-explore.md#test-data).
2. **`forge inspect [--json]`** (`crates/forge-cli/src/inspect.rs`) reports facts: imports and whether a file imports a UI framework, literal identifiers (with interpolated ones as prefixes, from the same reader as `forge ui focus`) and a note on what that misses, UI tests, baselines, generated files, the Rust crate, the iOS settings, agent files, and the one convention it can check exactly. `forge_inspect_app` runs it and adds the session's last failure. The MCP copy is deleted.
3. **`forge ui launch`** opens or stops the app, optionally in dark mode, at the largest text size or with other launch arguments. `forge_launch_app` runs it.
4. **Command history.** Every `forge` command on an app appends a line to `<app>/.forge/history.jsonl` (`forge.history/1`), in text and JSON mode, except under `forge mcp`. `forge verify --run` merges it with the MCP journal.
5. **Agent files are opt-in.** `forge agent setup [--apple-docs]` and `forge init --agent` write them from `templates/agent/` and never overwrite existing files. `forge_create_app` passes `--agent`. The agent instructions are shorter: easy-to-miss rules and the CLI commands, with nothing copied from the docs.
6. **The human-only workflow**: [human-workflow.md](../human-workflow.md) and `scripts/check-human-workflow.sh`, run in CI.

## Looked at and left alone

- **`forge ui focus` / `focus.rs`.** Every link already carries its basis (`declared` or `inferred`), the output says it is read as text and proves nothing, and the text output reads well (run on Good Dog's `ExerciseView.swift`). It is a CLI command and needs no MCP. The Rust-to-Swift name chain is the most speculative part. It is labelled, tested and not being extended, and removing it would discard working, labelled leads without evidence that they mislead. Leave it alone and don't grow it.
- **The explorer's core and its focus ordering.** Focus is a plain input (`Config::focus`) that changes only the order in which controls are tried. A walk without it is the plain seeded walk. There is no coupling to agents to remove.
- **`forge verify` and the acceptance boundary.** Twin, device and person columns never fill each other. `--accept` needs a terminal, the id typed back, and is refused under `forge mcp`. That is sound for its purpose. A program that fakes a terminal could still type it: the boundary stops an agent from certifying its own work by accident or through MCP, not a determined local program. That is acceptable for a developer tool and is now said here.
- **`forge_diagnose`'s correlation (`correlate.rs`) and `forge_device_status`.** Both are session state: the last failed tool call and the files changed since the server started. A person has `forge diagnose`, which starts from the app folder, plus `forge device apps` and `forge doctor`. Moving the correlation into the CLI would need a notion of a session that the CLI doesn't have. The command history (change 4) is the first piece of one, and the correlation could move once there is a second user.
- **`forge_ui_act` and `forge_screenshot`.** A person taps the phone; `forge ui screenshot` and `forge ui run` cover the rest. No tap or type command was added.
- **Building `forge-mcp` into every `forge` binary.** It costs compile time (rmcp, tokio) but nothing at run time, and the server only starts on `forge mcp`. A Cargo feature to leave it out is cheap if anyone asks for it.

## Compatibility

- Apps created before this change keep their `AGENTS.md`, `CLAUDE.md` and `.mcp.json`, and keep working. Nothing reads them. Their `.mcp.json` may still name the hosted sosumi server; delete that entry if you don't want it.
- To get the agent files in a new app: `forge agent setup`, or `forge init --agent`.
- `forge_inspect_app`'s `data` changed shape: `swift_files[].kind` became `imports_ui_framework`, and identifiers have `prefix`. Nothing in the repository read the old fields.
- `forge ui explore --real-data` explores less than before: anything that isn't navigation is skipped. List safe controls in `[ui_test] real_data_safe`.
- `report::finish` returns the result as well as the exit code. This is internal.

## Remaining gaps

- The iPhone steps ran on 2026-11-10 (`FORGE_HUMAN_DEVICE=1`, iPhone 15 Pro Max). Build, install, `forge ui launch`, screenshot, audit, the smoke test and `forge diagnose` passed, and a person saw the reading time on the phone. The run found that a laptop with no rustup default couldn't run `cargo` in the app folder (fixed: `forge init` puts `rust-toolchain.toml` there too). Still unproven on the phone: a 10-second log capture came back empty, and `forge ui explore --real-data` wasn't part of the run.
- `ui audit` warns on two of the starter's system controls: the search field has no identifier, and the New Note toolbar button is 90x33 pt.
- The real-data allow list is Linux-tested on the model app only.
- Text-mode commands record history but no log file. `--json` runs keep a log in `.forge/logs/`.
