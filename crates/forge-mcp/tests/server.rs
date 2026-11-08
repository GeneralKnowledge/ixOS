//! The MCP server end to end over an in-memory pipe, with a fake `forge`
//! (a shell script) standing in for the CLI, so no phone, Xcode SDK or
//! network is needed. The script records each call's arguments and answers
//! with canned `forge.result/1` objects.
#![cfg(unix)]

use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

const FAKE_FORGE: &str = r#"#!/bin/sh
# Records the call, then answers like `forge --json` would.
echo "$*" >> "$FAKE_LOG"
if [ "$1 $2" = "ui --help" ]; then [ -n "$FAKE_UI" ] && exit 0; exit 2; fi
shift # --json
case "$1 $2" in
  "doctor ") echo '{"schema_version":1,"ios_device":{"apple_sign_in":"logged_in","usbmuxd_running":true,"signing_team":{"id":"ABCDE12345","name":"Someone","kind":"free","sign_in":"apple_id"}},"ready":{"ios_device":true}}' ;;
  "device list") echo '{"schema":"forge.result/1","command":"device list","ok":true,"stage":"ok","duration_ms":5,"diagnostics":[],"hints":[],"data":{"devices":[{"udid":"0000","name":"Cati"}]}}' ;;
  "device logs") sleep 90 ;;
  "run ios") echo '{"schema":"forge.result/1","command":"run ios","ok":true,"stage":"ok","duration_ms":9,"diagnostics":[],"hints":[],"data":{"installed":true,"launched":false}}' ;;
  "build ios") echo "[forge] building"; echo '{"schema":"forge.result/1","command":"build ios","ok":false,"stage":"swift","duration_ms":7,"error":"xtool dev build failed","diagnostics":[{"severity":"error","source":"swiftc","file":"Sources/AppHost/TipView.swift","line":12,"column":5,"message":"cannot find tipModel in scope"}],"hints":[],"log":null}'; exit 11 ;;
  "ui start") [ -n "$FAKE_UI_START_FAILS" ] && { echo '{"schema":"forge.result/1","command":"ui start","ok":false,"stage":"device","duration_ms":4,"error":"no runner","diagnostics":[],"hints":[]}'; exit 1; }
    echo '{"schema":"forge.result/1","command":"ui start","ok":true,"stage":"ok","duration_ms":4,"diagnostics":[],"hints":[],"data":{"pid":1}}' ;;
  "ui explore") echo '{"schema":"forge.result/1","command":"ui explore","ok":false,"stage":"test","duration_ms":3,"error":"exploring found 1 error(s)","diagnostics":[],"hints":[],"data":{"seed":7,"issues":[{"number":1,"issue":"app_closed","severity":"error"}]}}'; exit 14 ;;
  "ui run") echo '{"schema":"forge.result/1","command":"ui run","ok":true,"stage":"ok","duration_ms":3,"diagnostics":[],"hints":[],"data":{"passed_steps":1}}' ;;
  "config show") echo 'not json at all'; exit 3 ;;
  *) echo "error: unrecognized subcommand '$1'" >&2; exit 2 ;;
esac
"#;

struct Fixture {
    _tmp: tempfile::TempDir,
    root: PathBuf,
    log: PathBuf,
    forge: PathBuf,
}

fn fixture() -> Fixture {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("root").canonicalize().unwrap_or_else(|_| {
        std::fs::create_dir(tmp.path().join("root")).unwrap();
        tmp.path().join("root").canonicalize().unwrap()
    });
    let app = root.join("Tipster");
    std::fs::create_dir_all(&app).unwrap();
    std::fs::write(
        app.join("forge.toml"),
        "[app]\nname = \"Tipster\"\nbundle_id = \"org.forgemobile.agentlab\"\n[ios]\nxtool_product = \"TipsterXtool\"\n",
    )
    .unwrap();
    let forge = tmp.path().join("forge");
    let mut f = std::fs::File::create(&forge).unwrap();
    f.write_all(FAKE_FORGE.as_bytes()).unwrap();
    drop(f);
    std::fs::set_permissions(&forge, std::fs::Permissions::from_mode(0o755)).unwrap();
    let log = tmp.path().join("calls.log");
    Fixture {
        root,
        log,
        forge,
        _tmp: tmp,
    }
}

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// A connected client: sends requests, returns responses by id.
struct Client {
    writer: tokio::io::WriteHalf<tokio::io::DuplexStream>,
    reader: tokio::io::Lines<BufReader<tokio::io::ReadHalf<tokio::io::DuplexStream>>>,
    next: u64,
}

impl Client {
    async fn start(fx: &Fixture, allow_install: bool, ui: bool) -> Client {
        Client::start_with(fx, allow_install, ui, false).await
    }

    async fn start_with(fx: &Fixture, allow_install: bool, ui: bool, no_device: bool) -> Client {
        std::env::set_var("FAKE_LOG", &fx.log);
        if ui {
            std::env::set_var("FAKE_UI", "1");
        } else {
            std::env::remove_var("FAKE_UI");
        }
        let opts = forge_mcp::Options {
            forge: fx.forge.clone(),
            root: fx.root.clone(),
            workspace: workspace(),
            allow_install,
            bundle_ids: vec!["org.forgemobile.agentlab".into()],
            udid: Some("0000".into()),
            no_device,
        };
        let (server_io, client_io) = tokio::io::duplex(1 << 20);
        tokio::spawn(async move {
            let _ = forge_mcp::serve_io(opts, server_io).await;
        });
        let (r, w) = tokio::io::split(client_io);
        let mut client = Client {
            writer: w,
            reader: BufReader::new(r).lines(),
            next: 1,
        };
        let init = client
            .request(
                "initialize",
                json!({"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "test", "version": "1"}}),
            )
            .await;
        assert_eq!(init["result"]["serverInfo"]["name"], "forge");
        client
            .send(json!({"jsonrpc": "2.0", "method": "notifications/initialized"}))
            .await;
        client
    }

    async fn send(&mut self, msg: Value) {
        let mut line = serde_json::to_string(&msg).unwrap();
        line.push('\n');
        self.writer.write_all(line.as_bytes()).await.unwrap();
    }

    async fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next;
        self.next += 1;
        self.send(json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}))
            .await;
        loop {
            let line =
                tokio::time::timeout(std::time::Duration::from_secs(60), self.reader.next_line())
                    .await
                    .expect("a response within 60 s")
                    .unwrap()
                    .expect("server still open");
            let msg: Value = serde_json::from_str(&line).unwrap();
            if msg["id"] == json!(id) {
                return msg;
            }
        }
    }

    async fn call(&mut self, tool: &str, args: Value) -> Value {
        let resp = self
            .request("tools/call", json!({"name": tool, "arguments": args}))
            .await;
        assert!(resp.get("error").is_none(), "{resp}");
        resp["result"].clone()
    }
}

fn calls(fx: &Fixture) -> Vec<String> {
    std::fs::read_to_string(&fx.log)
        .unwrap_or_default()
        .lines()
        .map(str::to_string)
        .collect()
}

// Tests share FAKE_LOG/FAKE_UI through the environment, so they run one at
// a time.
static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[tokio::test]
async fn tools_resources_and_prompts_are_listed() {
    let _g = SERIAL.lock().await;
    let fx = fixture();
    let mut c = Client::start(&fx, false, false).await;
    let tools = c.request("tools/list", json!({})).await;
    let names: Vec<&str> = tools["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"forge_build_ios") && names.contains(&"forge_install_device"));
    // This forge has no `ui`, so the UI tools are not offered.
    assert!(!names.contains(&"forge_inspect_ui"), "{names:?}");

    let res = c.request("resources/list", json!({})).await;
    let uris: Vec<&str> = res["result"]["resources"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["uri"].as_str().unwrap())
        .collect();
    assert!(uris.contains(&"forge://conventions") && uris.contains(&"forge://capabilities"));
    let conv = c
        .request("resources/read", json!({"uri": "forge://conventions"}))
        .await;
    assert!(conv["result"]["contents"][0]["text"]
        .as_str()
        .unwrap()
        .contains("Accessibility identifiers"));
    let missing = c
        .request(
            "resources/read",
            json!({"uri": "forge://docs/no-such-page"}),
        )
        .await;
    assert!(missing.get("error").is_some());

    let prompt = c
        .request(
            "prompts/get",
            json!({"name": "new_app", "arguments": {"name": "Tipster", "idea": "tips"}}),
        )
        .await;
    assert!(prompt["result"]["messages"][0]["content"]["text"]
        .as_str()
        .unwrap()
        .contains("forge_create_app"));
}

#[tokio::test]
async fn no_device_leaves_out_and_refuses_phone_tools() {
    let _g = SERIAL.lock().await;
    let fx = fixture();
    let mut c = Client::start_with(&fx, false, true, true).await;
    let tools = c.request("tools/list", json!({})).await;
    let names: Vec<&str> = tools["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"forge_build_ios") && names.contains(&"forge_check"));
    for phone in [
        "forge_install_device",
        "forge_device_status",
        "forge_screenshot",
        "forge_run_ui_test",
        "forge_ui_explore",
    ] {
        assert!(!names.contains(&phone), "{phone} listed: {names:?}");
    }
    let r = c
        .call("forge_screenshot", json!({"project": "Tipster"}))
        .await;
    assert_eq!(r["structuredContent"]["stage"], "policy", "{r}");
    // Only the startup probe for `forge ui` ran: nothing touched a phone.
    assert_eq!(calls(&fx), vec!["ui --help".to_string()]);
}

#[tokio::test]
async fn install_needs_the_persons_allowance() {
    let _g = SERIAL.lock().await;
    let fx = fixture();
    let mut c = Client::start(&fx, false, false).await;
    let refused = c
        .call("forge_install_device", json!({"project": "Tipster"}))
        .await;
    assert_eq!(refused["isError"], true);
    let s = &refused["structuredContent"];
    assert_eq!(s["stage"], "policy");
    assert_eq!(s["needs_person"], true);
    assert!(s["person_command"]
        .as_str()
        .unwrap()
        .starts_with("forge run ios --project"));
    assert!(
        calls(&fx).iter().all(|l| !l.contains("run ios")),
        "{:?}",
        calls(&fx)
    );

    let mut c = Client::start(&fx, true, false).await;
    let done = c
        .call("forge_install_device", json!({"project": "Tipster"}))
        .await;
    assert_eq!(done["isError"], false, "{done}");
    assert_eq!(done["structuredContent"]["data"]["installed"], true);
    assert!(calls(&fx)
        .iter()
        .any(|l| l.starts_with("--json run ios --project ")));
}

#[tokio::test]
async fn paths_outside_the_root_are_refused() {
    let _g = SERIAL.lock().await;
    let fx = fixture();
    let mut c = Client::start(&fx, true, false).await;
    for (tool, args) in [
        ("forge_build_ios", json!({"project": "../.."})),
        (
            "forge_create_app",
            json!({"name": "Escape", "path": "../Escape"}),
        ),
        ("forge_create_app", json!({"name": "Tipster"})),
    ] {
        let r = c.call(tool, args).await;
        assert_eq!(r["structuredContent"]["stage"], "usage", "{tool}: {r}");
    }
    assert!(calls(&fx)
        .iter()
        .all(|l| !l.contains("build ios") && !l.contains("init")));
}

#[tokio::test]
async fn failures_carry_diagnostics_and_feed_diagnose() {
    let _g = SERIAL.lock().await;
    let fx = fixture();
    let mut c = Client::start(&fx, false, false).await;
    let built = c
        .call("forge_build_ios", json!({"project": "Tipster"}))
        .await;
    assert_eq!(built["isError"], true);
    let summary = built["content"][0]["text"].as_str().unwrap();
    assert!(
        summary.contains("Sources/AppHost/TipView.swift:12:5: cannot find tipModel in scope"),
        "{summary}"
    );
    assert_eq!(built["structuredContent"]["stage"], "swift");

    // A command that printed no result is still a result.
    let shown = c
        .call("forge_inspect_app", json!({"project": "Tipster"}))
        .await;
    assert_eq!(shown["structuredContent"]["ok"], false);
    assert!(shown["structuredContent"]["error"]
        .as_str()
        .unwrap()
        .contains("printed no result"));

    let diag = c.call("forge_diagnose", json!({})).await;
    assert_eq!(
        diag["structuredContent"]["data"]["last_failure"]["command"],
        "config show"
    );
}

#[tokio::test]
async fn device_status_is_team_aware_and_hung_reads_are_stopped() {
    let _g = SERIAL.lock().await;
    let fx = fixture();
    let mut c = Client::start(&fx, false, false).await;
    let status = c.call("forge_device_status", json!({})).await;
    let data = &status["structuredContent"]["data"];
    assert_eq!(data["signing_team"]["kind"], "free");
    assert_eq!(data["devices"][0]["name"], "Cati");
    assert!(data["notes"][0]
        .as_str()
        .unwrap()
        .contains("3 sideloaded apps"));
    assert!(calls(&fx)
        .iter()
        .any(|l| l == "--json device list --udid 0000"));

    // The fake `device logs` hangs; seconds=1 gives it 31 s before the kill.
    let started = std::time::Instant::now();
    let logs = c
        .call(
            "forge_read_logs",
            json!({"project": "Tipster", "seconds": 1}),
        )
        .await;
    assert!(started.elapsed().as_secs() < 50);
    let s = &logs["structuredContent"];
    assert_eq!(s["stage"], "device");
    assert_eq!(s["needs_person"], true);
    assert!(
        calls(&fx)
            .iter()
            .any(|l| l
                .contains("device logs --for 1s --limit 200 --process TipsterXtool --udid 0000"))
    );
}

#[tokio::test]
async fn ui_actions_run_as_one_step_test_files() {
    let _g = SERIAL.lock().await;
    let fx = fixture();
    let mut c = Client::start(&fx, false, true).await;
    let tools = c.request("tools/list", json!({})).await;
    assert!(tools["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .any(|t| t["name"] == "forge_ui_act"));
    let r = c
        .call(
            "forge_ui_act",
            json!({"project": "Tipster", "action": "type", "id": "bill.amount", "text": "42"}),
        )
        .await;
    assert_eq!(r["isError"], false, "{r}");
    let line = calls(&fx)
        .into_iter()
        .find(|l| l.starts_with("--json ui run "))
        .unwrap();
    let file = line.split_whitespace().nth(3).unwrap();
    let script = std::fs::read_to_string(file).unwrap();
    assert!(
        script.contains("app = \"org.forgemobile.agentlab\""),
        "{script}"
    );
    assert!(script.contains("id = \"bill.amount\""), "{script}");
    assert!(script.contains("text = \"42\""), "{script}");
    let bad = c
        .call(
            "forge_ui_act",
            json!({"project": "Tipster", "action": "tap"}),
        )
        .await;
    assert_eq!(bad["structuredContent"]["stage"], "usage");

    // The run journal has both calls, the typed text only as its length.
    let runs = fx.root.join(".forge/runs");
    let dir = std::fs::read_dir(&runs)
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.to_string_lossy().ends_with("-mcp"))
        .expect("an -mcp run folder");
    let journal = std::fs::read_to_string(dir.join("journal.jsonl")).unwrap();
    let lines: Vec<Value> = journal
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(lines[0]["schema"], "forge.journal/1");
    assert_eq!(lines[0]["allow_install"], false);
    // The server kept one runner for both calls, tied to its own life.
    let starts: Vec<String> = calls(&fx)
        .into_iter()
        .filter(|l| l.starts_with("--json ui start "))
        .collect();
    assert_eq!(starts.len(), 2, "{starts:?}");
    assert_eq!(
        starts[0],
        format!(
            "--json ui start --idle-minutes 30 --exit-with {} --udid 0000",
            std::process::id()
        )
    );
    assert_eq!(lines[1]["tool"], "forge_ui_act");
    assert_eq!(lines[1]["ok"], true);
    assert_eq!(lines[1]["args"]["text"]["chars"], 2);
    assert_eq!(lines[2]["stage"], "usage");
}

#[tokio::test]
async fn a_failed_ui_session_start_is_not_retried_at_once() {
    let _g = SERIAL.lock().await;
    let fx = fixture();
    std::env::set_var("FAKE_UI_START_FAILS", "1");
    let mut c = Client::start(&fx, false, true).await;
    for _ in 0..2 {
        let r = c
            .call(
                "forge_ui_act",
                json!({"project": "Tipster", "action": "tap", "id": "bill.amount"}),
            )
            .await;
        // The tool still runs, with its own runner.
        assert_eq!(r["isError"], false, "{r}");
    }
    std::env::remove_var("FAKE_UI_START_FAILS");
    let calls = calls(&fx);
    let starts = calls.iter().filter(|l| l.contains(" ui start ")).count();
    let runs = calls.iter().filter(|l| l.contains(" ui run ")).count();
    assert_eq!((starts, runs), (1, 2), "{calls:?}");
}

#[tokio::test]
async fn explore_runs_on_test_data_and_never_allows_destructive_taps() {
    let _g = SERIAL.lock().await;
    let fx = fixture();
    let mut c = Client::start(&fx, false, true).await;
    let r = c
        .call(
            "forge_ui_explore",
            json!({"project": "Tipster", "actions": 500, "seed": 7, "skip": ["tip.reset"], "largest_text": true, "allow_destructive": true}),
        )
        .await;
    // allow_destructive isn't in the schema: refused as an unknown argument
    // or ignored, never passed on.
    let line = calls(&fx)
        .into_iter()
        .find(|l| l.starts_with("--json ui explore "))
        .unwrap_or_default();
    assert!(!line.contains("--allow-destructive"), "{line}");
    if line.is_empty() {
        assert_eq!(r["structuredContent"]["stage"], "usage", "{r}");
    }
    let r = c
        .call(
            "forge_ui_explore",
            json!({"project": "Tipster", "actions": 500, "seed": 7, "skip": ["tip.reset"], "largest_text": true}),
        )
        .await;
    assert_eq!(r["isError"], true, "{r}");
    let s = &r["structuredContent"];
    assert_eq!(s["stage"], "test");
    assert_eq!(s["data"]["issues"][0]["issue"], "app_closed");
    let out = s["data"]["out"].as_str().unwrap();
    assert!(
        out.starts_with("Tipster/.forge/runs/") && out.ends_with("-ui-explore"),
        "{out}"
    );
    let line = calls(&fx)
        .into_iter()
        .find(|l| l.starts_with("--json ui explore "))
        .unwrap();
    for part in [
        "--actions 200",
        "--minutes 5",
        "--per-screen 15",
        "--seed 7",
        "--skip tip.reset",
        "--largest-text",
        "--udid 0000",
    ] {
        assert!(line.contains(part), "{part} missing: {line}");
    }
    assert!(!line.contains("--allow-destructive") && !line.contains("--real-data"));
}
