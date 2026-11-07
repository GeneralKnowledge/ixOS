//! [`Driver`] over WebDriverAgent's HTTP API.
//!
//! Endpoints used (WebDriver plus WDA's own `/wda/...` routes):
//! `POST /session`, `POST /session/:s/elements` with a predicate,
//! `.../element/:e/click`, `.../element/:e/value`, `.../element/:e/clear`,
//! `GET .../element/:e/attribute/:name`, `POST .../wda/apps/{launch,activate,terminate}`,
//! `POST .../wda/deactivateApp`, `GET .../wda/activeAppInfo`, `GET /source?format=json`, `GET /screenshot`.

use base64::Engine as _;
use serde_json::{json, Value};

use crate::http::HttpTransport;
use crate::{Driver, ElementRef, Locator, Result, UiError, UiNode};

/// The W3C WebDriver key that carries an element id.
const ELEMENT_KEY: &str = "element-6066-11e4-a52e-4f735466cecf";

pub struct WdaDriver<T: HttpTransport> {
    transport: T,
    session: String,
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
        Ok(Self { transport, session })
    }

    /// WDA's `/status`: build info, and whether it is ready.
    pub fn status(&mut self) -> Result<Value> {
        value(call(&mut self.transport, "GET", "/status", None)?)
    }

    fn post(&mut self, path: &str, body: Value) -> Result<Value> {
        let path = format!("/session/{}{path}", self.session);
        value(call(&mut self.transport, "POST", &path, Some(&body))?)
    }

    fn get(&mut self, path: &str) -> Result<Value> {
        let path = format!("/session/{}{path}", self.session);
        value(call(&mut self.transport, "GET", &path, None)?)
    }
}

impl<T: HttpTransport> Driver for WdaDriver<T> {
    fn launch(&mut self, bundle_id: &str, args: &[String]) -> Result<()> {
        self.post(
            "/wda/apps/launch",
            json!({ "bundleId": bundle_id, "arguments": args, "environment": {} }),
        )?;
        Ok(())
    }

    fn activate(&mut self, bundle_id: &str) -> Result<()> {
        self.post("/wda/apps/activate", json!({ "bundleId": bundle_id }))?;
        Ok(())
    }

    fn terminate(&mut self, bundle_id: &str) -> Result<()> {
        self.post("/wda/apps/terminate", json!({ "bundleId": bundle_id }))?;
        Ok(())
    }

    fn background(&mut self, seconds: f64) -> Result<()> {
        self.post("/wda/deactivateApp", json!({ "duration": seconds }))?;
        Ok(())
    }

    fn find_all(&mut self, locator: &Locator) -> Result<Vec<ElementRef>> {
        let found = self.post(
            "/elements",
            json!({ "using": "predicate string", "value": locator.predicate() }),
        )?;
        let list = found
            .as_array()
            .ok_or_else(|| UiError::Wda("element list is not an array".into()))?;
        list.iter()
            .map(|item| {
                item.get(ELEMENT_KEY)
                    .or_else(|| item.get("ELEMENT"))
                    .and_then(Value::as_str)
                    .map(|id| ElementRef(id.to_owned()))
                    .ok_or_else(|| UiError::Wda("element without an id".into()))
            })
            .collect()
    }

    fn tap(&mut self, element: &ElementRef) -> Result<()> {
        self.post(&format!("/element/{}/click", element.0), json!({}))?;
        Ok(())
    }

    fn type_text(&mut self, element: &ElementRef, text: &str) -> Result<()> {
        let chars: Vec<String> = text.chars().map(String::from).collect();
        self.post(
            &format!("/element/{}/value", element.0),
            json!({ "value": chars, "text": text }),
        )?;
        Ok(())
    }

    fn clear(&mut self, element: &ElementRef) -> Result<()> {
        self.post(&format!("/element/{}/clear", element.0), json!({}))?;
        Ok(())
    }

    fn attribute(&mut self, element: &ElementRef, name: &str) -> Result<Option<String>> {
        let value = self.get(&format!("/element/{}/attribute/{name}", element.0))?;
        Ok(match value {
            Value::Null => None,
            Value::String(s) if s.is_empty() => None,
            Value::String(s) => Some(s),
            other => Some(other.to_string()),
        })
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
        let source = value(call(
            &mut self.transport,
            "GET",
            "/source?format=json",
            None,
        )?)?;
        Ok(UiNode::from_wda(&source))
    }

    fn screenshot(&mut self) -> Result<Vec<u8>> {
        let encoded = value(call(&mut self.transport, "GET", "/screenshot", None)?)?;
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
                r#"POST /session/S1/elements {"using":"predicate string","value":"name == \"bookForm.title\""}"#,
                "POST /session/S1/element/E7/click {}",
                r#"POST /session/S1/element/E7/value {"text":"Hi","value":["H","i"]}"#,
                "GET /session/S1/element/E7/attribute/value",
            ]
        );
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
