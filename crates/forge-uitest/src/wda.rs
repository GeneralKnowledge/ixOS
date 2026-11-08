//! [`Driver`] over WebDriverAgent's HTTP API.
//!
//! Endpoints used (WebDriver plus WDA's own `/wda/...` routes):
//! `POST /session`, `POST /session/:s/elements` with a predicate,
//! `.../element/:e/click`, `.../element/:e/value`, `.../element/:e/clear`,
//! `GET .../element/:e/attribute/:name`, `POST .../wda/apps/{launch,activate,terminate}`,
//! `POST .../wda/deactivateApp`, `POST .../wda/element/:e/scroll`, `GET .../wda/activeAppInfo`, `GET /source?format=json`, `GET /screenshot`.

use std::collections::HashMap;
use std::time::Instant;

use base64::Engine as _;
use serde_json::{json, Value};

use crate::http::HttpTransport;
use crate::{Driver, ElementRef, Locator, Result, UiError, UiNode};

/// The W3C WebDriver key that carries an element id.
const ELEMENT_KEY: &str = "element-6066-11e4-a52e-4f735466cecf";

/// The attributes a search answers with when [`Driver::describe_matches`]
/// is on, as WDA's `elementResponseAttributes` setting names them, and the
/// attribute each one stands for. `attribute/name` is the identifier: in an
/// element reply WDA's plain `name` field is the W3C tag name (the type).
const DESCRIBED: [(&str, &str); 3] = [
    ("type", "type"),
    ("label", "label"),
    ("attribute/name", "name"),
];

pub struct WdaDriver<T: HttpTransport> {
    transport: T,
    session: String,
    /// What the last search said about each element it found: type, label
    /// and identifier, read from the snapshot WDA took for the search.
    /// Emptied by every search and by anything that can change the screen.
    described: HashMap<String, HashMap<&'static str, Option<String>>>,
    /// Print each request's time to stderr (`FORGE_UI_TIMINGS=1`).
    timings: bool,
}

impl<T: HttpTransport> WdaDriver<T> {
    /// Opens a WebDriver session on a running WebDriverAgent.
    pub fn connect(mut transport: T) -> Result<Self> {
        let body = json!({ "capabilities": { "alwaysMatch": {} } });
        let reply = call(&mut transport, "POST", "/session", Some(&body))?;
        let session = reply
            .get("sessionId")
            .or_else(|| reply.get("value").and_then(|v| v.get("sessionId")))
            .and_then(Value::as_str)
            .ok_or_else(|| UiError::Wda("no session id in reply".into()))?
            .to_owned();
        let timings =
            std::env::var_os("FORGE_UI_TIMINGS").is_some_and(|v| !v.is_empty() && v != "0");
        Ok(Self {
            transport,
            session,
            described: HashMap::new(),
            timings,
        })
    }

    /// WDA's `/status`: build info, and whether it is ready.
    pub fn status(&mut self) -> Result<Value> {
        value(call(&mut self.transport, "GET", "/status", None)?)
    }

    fn post(&mut self, path: &str, body: Value) -> Result<Value> {
        let path = format!("/session/{}{path}", self.session);
        self.timed("POST", &path, Some(&body))
    }

    fn get(&mut self, path: &str) -> Result<Value> {
        let path = format!("/session/{}{path}", self.session);
        self.timed("GET", &path, None)
    }

    /// One request, timed on stderr when `FORGE_UI_TIMINGS` is set. Element
    /// ids are left out of the line; typed text never appears in it.
    fn timed(&mut self, method: &str, path: &str, body: Option<&Value>) -> Result<Value> {
        let started = Instant::now();
        let result = call(&mut self.transport, method, path, body).and_then(value);
        if self.timings {
            let route = route_of(path, &self.session);
            let ms = started.elapsed().as_millis();
            let outcome = if result.is_ok() { "" } else { " (error)" };
            eprintln!("          wda {ms:>5} ms  {method} {route}{outcome}");
        }
        result
    }

    /// Every element WDA's query for the locator returns, in tree order.
    fn query(&mut self, locator: &Locator) -> Result<Vec<ElementRef>> {
        // Ids use WDA's `accessibility id` strategy: XCTest's own identifier
        // query, which Appium documents as the fastest locator, instead of a
        // predicate evaluated over the whole tree. Same matching as `name`.
        let query = match locator {
            Locator::Id(id) => json!({ "using": "accessibility id", "value": id }),
            _ => json!({ "using": "predicate string", "value": locator.predicate() }),
        };
        let found = match self.post("/elements", query) {
            Err(UiError::Wda(_)) if matches!(locator, Locator::Id(_)) => self.post(
                "/elements",
                json!({ "using": "predicate string", "value": locator.predicate() }),
            )?,
            other => other?,
        };
        let list = found
            .as_array()
            .ok_or_else(|| UiError::Wda("element list is not an array".into()))?;
        self.described.clear();
        list.iter()
            .map(|item| {
                let id = item
                    .get(ELEMENT_KEY)
                    .or_else(|| item.get("ELEMENT"))
                    .and_then(Value::as_str)
                    .ok_or_else(|| UiError::Wda("element without an id".into()))?;
                // Kept only if the reply has every described field: a
                // compact reply (describing off) has none of them.
                let described: Option<HashMap<_, _>> = DESCRIBED
                    .iter()
                    .map(|(field, attribute)| {
                        item.get(*field).map(|v| (*attribute, attribute_text(v)))
                    })
                    .collect();
                if let Some(described) = described {
                    self.described.insert(id.to_owned(), described);
                }
                Ok(ElementRef(id.to_owned()))
            })
            .collect()
    }

    /// A [`Locator::Near`]: the nearest control is chosen from the tree
    /// (it needs frames), then reached as "the nth element of its kind" by
    /// a query. If the query doesn't return as many of that kind as the
    /// tree had, the screen changed in between: stale, and the caller asks
    /// again.
    fn find_near(&mut self, locator: &Locator) -> Result<Vec<ElementRef>> {
        let tree = self.tree()?;
        let Some((at, node)) = locator.find_in(&tree).into_iter().next() else {
            return Ok(Vec::new());
        };
        let of_kind = Locator::Kind {
            kind: node.kind.clone(),
            label: None,
            index: None,
        };
        let in_tree: Vec<usize> = of_kind.find_in(&tree).iter().map(|(i, _)| *i).collect();
        let index = in_tree
            .iter()
            .position(|i| *i == at)
            .ok_or_else(|| UiError::Wda(format!("{locator} is not of its own kind")))?;
        let found = self.query(&of_kind)?;
        if found.len() != in_tree.len() {
            return Err(UiError::Stale(format!(
                "the screen changed while finding {locator}: {} {} in the tree, {} by query",
                in_tree.len(),
                node.kind,
                found.len()
            )));
        }
        Ok(vec![found[index].clone()])
    }

    /// Something may have changed on screen: forget what searches said.
    fn changed(&mut self) {
        self.described.clear();
    }
}

/// A request path without its session and element ids, for timing lines:
/// `/session/S/element/E/value` → `/element/:e/value`.
fn route_of(path: &str, session: &str) -> String {
    let path = path
        .strip_prefix(&format!("/session/{session}"))
        .unwrap_or(path);
    let mut parts: Vec<&str> = path.split('/').collect();
    for i in 1..parts.len() {
        if parts[i - 1] == "element" && !parts[i].is_empty() {
            parts[i] = ":e";
        }
    }
    let route = parts.join("/");
    if route.is_empty() {
        "/".into()
    } else {
        route
    }
}

/// An attribute as [`Driver::attribute`] returns it: empty text is no text.
fn attribute_text(value: &Value) -> Option<String> {
    match value {
        Value::Null => None,
        Value::String(s) if s.is_empty() => None,
        Value::String(s) => Some(s.clone()),
        other => Some(other.to_string()),
    }
}

impl<T: HttpTransport> Driver for WdaDriver<T> {
    fn launch(&mut self, bundle_id: &str, args: &[String]) -> Result<()> {
        self.changed();
        self.post(
            "/wda/apps/launch",
            json!({ "bundleId": bundle_id, "arguments": args, "environment": {} }),
        )?;
        Ok(())
    }

    fn activate(&mut self, bundle_id: &str) -> Result<()> {
        self.changed();
        self.post("/wda/apps/activate", json!({ "bundleId": bundle_id }))?;
        Ok(())
    }

    fn terminate(&mut self, bundle_id: &str) -> Result<()> {
        self.changed();
        self.post("/wda/apps/terminate", json!({ "bundleId": bundle_id }))?;
        Ok(())
    }

    fn background(&mut self, seconds: f64) -> Result<()> {
        self.changed();
        self.post("/wda/deactivateApp", json!({ "duration": seconds }))?;
        Ok(())
    }

    fn find_all(&mut self, locator: &Locator) -> Result<Vec<ElementRef>> {
        if locator.needs_tree() {
            return self.find_near(locator);
        }
        let found = self.query(locator)?;
        // A `Kind` with an index: that one of the matches, in tree order
        // (XCTest's query order, the same as `/source`'s).
        Ok(match locator {
            Locator::Kind {
                index: Some(index), ..
            } => found.into_iter().skip(*index).take(1).collect(),
            _ => found,
        })
    }

    fn describe_matches(&mut self, on: bool) -> Result<()> {
        let fields: Vec<&str> = DESCRIBED.iter().map(|(field, _)| *field).collect();
        let mut settings = json!({ "shouldUseCompactResponses": !on });
        if on {
            settings["elementResponseAttributes"] = json!(fields.join(","));
        }
        // For measuring on a phone: more WDA settings to try, such as
        // `{"waitForIdleTimeout": 1}`. Not a supported knob.
        if let Some(extra) = std::env::var_os("FORGE_UI_WDA_SETTINGS") {
            match serde_json::from_str::<Value>(&extra.to_string_lossy()) {
                Ok(Value::Object(extra)) => {
                    for (key, value) in extra {
                        settings[key] = value;
                    }
                }
                _ => eprintln!("          FORGE_UI_WDA_SETTINGS is not a JSON object; ignored"),
            }
        }
        self.post("/appium/settings", json!({ "settings": settings }))?;
        Ok(())
    }

    fn scroll(&mut self, element: &ElementRef, direction: &str) -> Result<()> {
        self.changed();
        let body = if direction == "visible" {
            json!({ "toVisible": true })
        } else {
            json!({ "direction": direction })
        };
        self.post(&format!("/wda/element/{}/scroll", element.0), body)?;
        Ok(())
    }

    fn tap(&mut self, element: &ElementRef) -> Result<()> {
        self.changed();
        self.post(&format!("/element/{}/click", element.0), json!({}))?;
        Ok(())
    }

    fn type_text(&mut self, element: &ElementRef, text: &str) -> Result<()> {
        self.changed();
        let chars: Vec<String> = text.chars().map(String::from).collect();
        self.post(
            &format!("/element/{}/value", element.0),
            json!({ "value": chars, "text": text }),
        )?;
        Ok(())
    }

    fn clear(&mut self, element: &ElementRef) -> Result<()> {
        self.changed();
        self.post(&format!("/element/{}/clear", element.0), json!({}))?;
        Ok(())
    }

    fn attribute(&mut self, element: &ElementRef, name: &str) -> Result<Option<String>> {
        // Type, label and identifier as the search that found the element
        // saw them: no round trip, and no second snapshot of the whole app.
        if let Some(known) = self.described.get(&element.0).and_then(|d| d.get(name)) {
            return Ok(known.clone());
        }
        let value = self.get(&format!("/element/{}/attribute/{name}", element.0))?;
        Ok(attribute_text(&value))
    }

    fn foreground(&mut self) -> Result<Option<String>> {
        let info = self.get("/wda/activeAppInfo")?;
        Ok(info
            .get("bundleId")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_owned))
    }

    fn tree(&mut self) -> Result<UiNode> {
        let source = self.timed("GET", "/source?format=json", None)?;
        Ok(UiNode::from_wda(&source))
    }

    fn screenshot(&mut self) -> Result<Vec<u8>> {
        let encoded = self.timed("GET", "/screenshot", None)?;
        let encoded = encoded
            .as_str()
            .ok_or_else(|| UiError::Wda("screenshot is not a string".into()))?;
        // WDA wraps long base64 lines on some versions.
        let compact: String = encoded.chars().filter(|c| !c.is_whitespace()).collect();
        base64::engine::general_purpose::STANDARD
            .decode(compact)
            .map_err(|e| UiError::Wda(format!("screenshot is not base64: {e}")))
    }
}

/// One request; JSON in, JSON out, WebDriver errors turned into [`UiError`].
fn call<T: HttpTransport>(
    transport: &mut T,
    method: &str,
    path: &str,
    body: Option<&Value>,
) -> Result<Value> {
    let bytes = body.map(|b| b.to_string().into_bytes());
    let (status, reply) = transport.request(method, path, bytes.as_deref())?;
    let json: Value = serde_json::from_slice(&reply).map_err(|_| {
        UiError::Wda(format!(
            "{method} {path}: HTTP {status}, not JSON: {}",
            String::from_utf8_lossy(&reply)
                .chars()
                .take(200)
                .collect::<String>()
        ))
    })?;
    let error = json
        .get("value")
        .and_then(|v| v.get("error"))
        .and_then(Value::as_str);
    if status >= 400 || error.is_some() {
        let message = json
            .get("value")
            .and_then(|v| v.get("message"))
            .and_then(Value::as_str)
            .unwrap_or("");
        let text = format!(
            "{method} {path}: {} {}",
            error.unwrap_or("HTTP error"),
            message.lines().next().unwrap_or("")
        );
        if error == Some("stale element reference") {
            return Err(UiError::Stale(text));
        }
        return Err(UiError::Wda(text));
    }
    Ok(json)
}

fn value(reply: Value) -> Result<Value> {
    match reply {
        Value::Object(mut map) => Ok(map.remove("value").unwrap_or(Value::Null)),
        _ => Err(UiError::Wda("reply is not an object".into())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    /// Replays canned replies and records the requests.
    #[derive(Default)]
    struct Script {
        replies: VecDeque<(u16, Value)>,
        sent: Vec<String>,
    }

    impl HttpTransport for &mut Script {
        fn request(
            &mut self,
            method: &str,
            path: &str,
            body: Option<&[u8]>,
        ) -> Result<(u16, Vec<u8>)> {
            let body = body
                .map(|b| String::from_utf8_lossy(b).into_owned())
                .unwrap_or_default();
            self.sent
                .push(format!("{method} {path} {body}").trim_end().to_owned());
            let (status, reply) = self.replies.pop_front().expect("unexpected request");
            Ok((status, reply.to_string().into_bytes()))
        }
    }

    fn script(replies: Vec<(u16, Value)>) -> Script {
        Script {
            replies: replies.into(),
            sent: Vec::new(),
        }
    }

    #[test]
    fn finds_by_identifier_taps_and_types() {
        let mut s = script(vec![
            (
                200,
                json!({ "value": { "sessionId": "S1" }, "sessionId": "S1" }),
            ),
            (
                200,
                json!({ "value": [{ ELEMENT_KEY: "E7", "ELEMENT": "E7" }] }),
            ),
            (200, json!({ "value": null })),
            (200, json!({ "value": null })),
            (200, json!({ "value": "Hello" })),
        ]);
        let mut driver = WdaDriver::connect(&mut s).unwrap();
        let found = driver
            .find_all(&Locator::Id("bookForm.title".into()))
            .unwrap();
        assert_eq!(found, [ElementRef("E7".into())]);
        driver.tap(&found[0]).unwrap();
        driver.type_text(&found[0], "Hi").unwrap();
        assert_eq!(
            driver.attribute(&found[0], "value").unwrap().as_deref(),
            Some("Hello")
        );
        drop(driver);
        assert_eq!(
            s.sent,
            [
                r#"POST /session {"capabilities":{"alwaysMatch":{}}}"#,
                r#"POST /session/S1/elements {"using":"accessibility id","value":"bookForm.title"}"#,
                "POST /session/S1/element/E7/click {}",
                r#"POST /session/S1/element/E7/value {"text":"Hi","value":["H","i"]}"#,
                "GET /session/S1/element/E7/attribute/value",
            ]
        );
    }

    #[test]
    fn searches_describe_matches_so_policy_reads_need_no_round_trip() {
        let mut s = script(vec![
            (200, json!({ "sessionId": "S1", "value": {} })),
            (200, json!({ "value": {} })),
            (
                200,
                json!({ "value": [{
                    ELEMENT_KEY: "E7",
                    "type": "XCUIElementTypeTextView",
                    "label": "",
                    "attribute/name": "chat.input",
                }] }),
            ),
            (200, json!({ "value": null })),
            (200, json!({ "value": "Write a message" })),
        ]);
        let mut driver = WdaDriver::connect(&mut s).unwrap();
        driver.describe_matches(true).unwrap();
        let found = driver.find_all(&Locator::Id("chat.input".into())).unwrap();
        let e = &found[0];
        // Answered from the search: no requests.
        assert_eq!(
            driver.attribute(e, "type").unwrap().as_deref(),
            Some("XCUIElementTypeTextView")
        );
        assert_eq!(driver.attribute(e, "label").unwrap(), None);
        assert_eq!(
            driver.attribute(e, "name").unwrap().as_deref(),
            Some("chat.input")
        );
        // Typing can change the screen: the label is asked for again.
        driver.type_text(e, "Hi").unwrap();
        assert_eq!(
            driver.attribute(e, "label").unwrap().as_deref(),
            Some("Write a message")
        );
        drop(driver);
        assert_eq!(
            s.sent,
            [
                r#"POST /session {"capabilities":{"alwaysMatch":{}}}"#,
                r#"POST /session/S1/appium/settings {"settings":{"elementResponseAttributes":"type,label,attribute/name","shouldUseCompactResponses":false}}"#,
                r#"POST /session/S1/elements {"using":"accessibility id","value":"chat.input"}"#,
                r#"POST /session/S1/element/E7/value {"text":"Hi","value":["H","i"]}"#,
                "GET /session/S1/element/E7/attribute/label",
            ]
        );
    }

    #[test]
    fn compact_replies_and_other_attributes_still_ask_the_phone() {
        let mut s = script(vec![
            (200, json!({ "sessionId": "S1", "value": {} })),
            (200, json!({ "value": {} })),
            // A runner that ignored the setting: ids only.
            (200, json!({ "value": [{ ELEMENT_KEY: "E1" }] })),
            (200, json!({ "value": "Send" })),
            (200, json!({ "value": true })),
        ]);
        let mut driver = WdaDriver::connect(&mut s).unwrap();
        driver.describe_matches(false).unwrap();
        let found = driver.find_all(&Locator::Id("chat.send".into())).unwrap();
        assert_eq!(
            driver.attribute(&found[0], "label").unwrap().as_deref(),
            Some("Send")
        );
        assert_eq!(
            driver.attribute(&found[0], "hittable").unwrap().as_deref(),
            Some("true")
        );
        drop(driver);
        assert_eq!(
            s.sent[1],
            r#"POST /session/S1/appium/settings {"settings":{"shouldUseCompactResponses":true}}"#
        );
        assert_eq!(s.sent.len(), 5);
    }

    #[test]
    fn timing_lines_leave_out_ids() {
        assert_eq!(
            route_of("/session/S1/element/E7/value", "S1"),
            "/element/:e/value"
        );
        assert_eq!(
            route_of("/session/S1/wda/element/E7/scroll", "S1"),
            "/wda/element/:e/scroll"
        );
        assert_eq!(route_of("/session/S1/elements", "S1"), "/elements");
        assert_eq!(route_of("/status", "S1"), "/status");
    }

    #[test]
    fn falls_back_to_a_predicate_if_accessibility_id_is_refused() {
        let mut s = script(vec![
            (200, json!({ "sessionId": "S1", "value": {} })),
            (
                400,
                json!({ "value": { "error": "invalid selector", "message": "unsupported" } }),
            ),
            (200, json!({ "value": [{ ELEMENT_KEY: "E1" }] })),
        ]);
        let mut driver = WdaDriver::connect(&mut s).unwrap();
        let found = driver.find_all(&Locator::Id("chat.send".into())).unwrap();
        assert_eq!(found, [ElementRef("E1".into())]);
        drop(driver);
        assert!(
            s.sent[2].contains(r#""using":"predicate string""#),
            "{}",
            s.sent[2]
        );
    }

    /// WDA's JSON source for an element.
    fn wda_node(kind: &str, label: Option<&str>, rect: [f64; 4], children: Vec<Value>) -> Value {
        json!({
            "type": format!("XCUIElementType{kind}"),
            "label": label,
            "isEnabled": "1",
            "isVisible": "1",
            "rect": { "x": rect[0], "y": rect[1], "width": rect[2], "height": rect[3] },
            "children": children,
        })
    }

    #[test]
    fn finds_a_field_by_its_placeholder_as_the_nth_of_its_kind() {
        // Two text views: the second has the placeholder over it.
        let source = wda_node(
            "Application",
            None,
            [0.0, 0.0, 430.0, 932.0],
            vec![
                wda_node("TextView", None, [16.0, 100.0, 398.0, 300.0], vec![]),
                wda_node("TextView", None, [16.0, 790.0, 398.0, 56.0], vec![]),
                wda_node(
                    "StaticText",
                    Some("Message or hold to speak"),
                    [22.0, 795.0, 386.0, 45.0],
                    vec![],
                ),
            ],
        );
        let near = Locator::Near {
            text: "Message or hold to speak".into(),
            kind: None,
        };
        let mut s = script(vec![
            (200, json!({ "sessionId": "S1", "value": {} })),
            (200, json!({ "value": source })),
            (
                200,
                json!({ "value": [{ ELEMENT_KEY: "T1" }, { ELEMENT_KEY: "T2" }] }),
            ),
            // The screen changed: one text view by query, two in the tree.
            (200, json!({ "value": source })),
            (200, json!({ "value": [{ ELEMENT_KEY: "T1" }] })),
        ]);
        let mut driver = WdaDriver::connect(&mut s).unwrap();
        assert_eq!(driver.find_all(&near).unwrap(), [ElementRef("T2".into())]);
        assert!(matches!(driver.find_all(&near), Err(UiError::Stale(_))));
        drop(driver);
        assert_eq!(s.sent[1], "GET /source?format=json");
        assert_eq!(
            s.sent[2],
            r#"POST /session/S1/elements {"using":"predicate string","value":"type == \"XCUIElementTypeTextView\""}"#
        );
    }

    #[test]
    fn a_kind_with_an_index_takes_that_match() {
        let mut s = script(vec![
            (200, json!({ "sessionId": "S1", "value": {} })),
            (
                200,
                json!({ "value": [{ ELEMENT_KEY: "B1" }, { ELEMENT_KEY: "B2" }, { ELEMENT_KEY: "B3" }] }),
            ),
        ]);
        let mut driver = WdaDriver::connect(&mut s).unwrap();
        let third = Locator::Kind {
            kind: "Button".into(),
            label: None,
            index: Some(2),
        };
        assert_eq!(driver.find_all(&third).unwrap(), [ElementRef("B3".into())]);
    }

    #[test]
    fn webdriver_errors_become_messages() {
        let mut s = script(vec![
            (200, json!({ "sessionId": "S1", "value": {} })),
            (
                404,
                json!({ "value": { "error": "no such element", "message": "unable to find\nstack" } }),
            ),
        ]);
        let mut driver = WdaDriver::connect(&mut s).unwrap();
        let err = driver.tap(&ElementRef("gone".into())).unwrap_err();
        assert_eq!(
            err.to_string(),
            "WebDriverAgent: POST /session/S1/element/gone/click: no such element unable to find"
        );
    }

    #[test]
    fn stale_elements_are_their_own_error() {
        let mut s = script(vec![
            (200, json!({ "sessionId": "S1", "value": {} })),
            (
                404,
                json!({ "value": { "error": "stale element reference", "message": "gone" } }),
            ),
        ]);
        let mut driver = WdaDriver::connect(&mut s).unwrap();
        assert!(matches!(
            driver.tap(&ElementRef("E1".into())),
            Err(UiError::Stale(_))
        ));
    }

    #[test]
    fn decodes_screenshots() {
        let mut s = script(vec![
            (200, json!({ "sessionId": "S1", "value": {} })),
            (200, json!({ "value": "iVBO\nRw==" })),
        ]);
        let mut driver = WdaDriver::connect(&mut s).unwrap();
        assert_eq!(driver.screenshot().unwrap(), [0x89, b'P', b'N', b'G']);
    }
}
