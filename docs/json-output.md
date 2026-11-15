# `--json`: one result object per command

Add `--json` to any `forge` command and stdout carries exactly one JSON object, the result. Progress and raw tool output go to stderr and to a log file the result points at. Scripts, CI and tools built on forge (such as the MCP server in the [forge-lab](https://github.com/GeneralKnowledge/forge-lab) side project) read this object instead of parsing text. The code is `crates/forge-cli/src/report.rs`.

```json
{
  "schema": "forge.result/1",
  "command": "run ios",
  "ok": false,
  "stage": "swift",
  "duration_ms": 41210,
  "error": "the Swift build failed",
  "diagnostics": [
    {"severity": "error", "source": "swiftc", "file": "Sources/AppHost/TimelineView.swift",
     "line": 42, "column": 17, "message": "value of type 'Post' has no member 'likeCount'"}
  ],
  "hints": [
    {"stage": "swift", "problem": "Swift calls a Rust API that changed.",
     "fix": "Run `forge bindings generate`, then rebuild."}
  ],
  "device": {"udid": "…", "name": "Cati"},
  "app": {"name": "Shelf", "bundle_id": "org.example.shelf", "product": "ShelfXtool", "path": "…"},
  "data": {},
  "log": ".forge/logs/2026-11-07T16-40-12-run-ios.log"
}
```

## Fields

| Field | What it holds |
|---|---|
| `schema` | Always `forge.result/1` for this shape |
| `command` | The command, as `run ios`, `device logs`, `ui tap` |
| `ok` | Whether it succeeded |
| `stage` | Where it stopped: `ok`, `rust`, `swift`, `signing`, `device`, `test`, `usage` or `forge` |
| `error` | The human-readable message, only on failure |
| `diagnostics` | Compiler errors and warnings, with file, line and column when known |
| `hints` | What `diagnose.rs` recognised in the output: the problem and the fix |
| `device` | The phone the command used, when it used one |
| `app` | The app, whenever the command works on an app directory: name, bundle id, xtool product and path |
| `data` | The command's own payload: `init` gives the path, `config show` the effective settings, `device list` and `info` the phones, `device crashes` the reports and pulled files, `device logs` `data.lines`, `run ios` `installed` and `launched` |
| `log` | `<app>/.forge/logs/<UTC time>-<command>.log`, written for `build ios`, `run ios` and `test`, each step starting with `$ <command line>` |

## Exit codes

The exit code follows `stage`, so a shell script can branch without parsing:

| Code | Stage |
|---|---|
| 0 | `ok` |
| 1 | `forge`: anything unclassified, such as a bad `forge.toml` |
| 2 | `usage`: the command was called wrongly, for example `device logs --json` with neither `--for` nor `--limit` |
| 10 | `rust` |
| 11 | `swift` |
| 12 | `signing` |
| 13 | `device`; device commands that fail without a recognised cause count here |
| 14 | `test`; `forge test`, `forge check`, `forge verify` and `forge ui run` count here |

## Notes

- Raw output is never dropped: it is in the log file.
- `forge doctor --json` keeps its own report shape, which predates this contract.
- `forge device logs --for <DURATION>` ends a log read even when the phone is silent. The USB syslog relay is live-only, so there is no `--since` that reaches back in time.
