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
  "device list") echo '{"schema":"forge.result/1","command":"device list","ok":true,"stage":"ok","duration_ms":5,"diagnostics":[],"hints":[],"data":[{"udid":"0000","name":"Cati"}]}' ;;
  "device logs") sleep 90 ;;
  "run ios") echo '{"schema":"forge.result/1","command":"run ios","ok":true,"stage":"ok","duration_ms":9,"diagnostics":[],"hints":[],"data":{"installed":true,"launched":false}}' ;;
  "build ios") echo "[forge] building"; echo '{"schema":"forge.result/1","command":"build ios","ok":false,"stage":"swift","duration_ms":7,"error":"xtool dev build failed","diagnostics":[{"severity":"error","source":"swiftc","file":"Sources/AppHost/TipView.swift","line":12,"column":5,"message":"cannot find tipModel in scope"}],"hints":[],"log":null}'; exit 11 ;;
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
}
