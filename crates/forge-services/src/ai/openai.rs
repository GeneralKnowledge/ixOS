//! A model behind an OpenAI-compatible server: llama.cpp's `llama-server`,
//! Ollama, LM Studio, vLLM and others all answer `POST
//! <base>/chat/completions`, and stream with server-sent events when asked.
//!
//! Requests go through any `forge_net::HttpClient` (`UreqClient` on Linux,
//! the host's URLSession on iOS). Nothing here keeps the text: it is read,
//! pushed into the caller's [`TextOut`] and forgotten.

use std::io::{BufRead, BufReader};
use std::sync::Arc;
use std::time::Duration;

use forge_net::{CancelHandle, HttpClient, NetError, Request};
use serde_json::{json, Value};

use super::{AiError, ChatRequest, LanguageModel, ModelInfo, TextOut};

/// How long to wait for the server to start answering (it may be loading
/// the model, or reading a long prompt on a CPU).
pub const FIRST_ANSWER_TIMEOUT: Duration = Duration::from_secs(300);

/// A server's model.
pub struct OpenAiServer {
    client: Arc<dyn HttpClient>,
    base_url: String,
    model: Option<String>,
    api_key: Option<String>,
    context_tokens: u32,
}

impl OpenAiServer {
    /// `base_url` ends in `/v1` for most servers
    /// (`http://127.0.0.1:11434/v1` for Ollama).
    pub fn new(client: Arc<dyn HttpClient>, base_url: impl Into<String>) -> Self {
        Self {
            client,
            base_url: base_url.into().trim_end_matches('/').to_string(),
            model: None,
            api_key: None,
            context_tokens: 8192,
        }
    }

    /// The model to ask when a request names none.
    pub fn model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    /// Sent as a bearer token, for servers that want one.
    pub fn api_key(mut self, key: impl Into<String>) -> Self {
        self.api_key = Some(key.into());
        self
    }

    /// What the server's model reads and writes in one request (the servers
    /// don't say; 8192 unless set).
    pub fn context_tokens(mut self, tokens: u32) -> Self {
        self.context_tokens = tokens;
        self
    }

    /// The model it asks by default, if one was set.
    pub fn default_model(&self) -> Option<&str> {
        self.model.as_deref()
    }

    /// The models the server offers (`GET <base>/models`).
    pub fn models(&self) -> Result<Vec<String>, AiError> {
        let request = self.authorised(Request::get(format!("{}/models", self.base_url)));
        let response = self.client.send(&request).map_err(unreachable)?;
        if !response.is_success() {
            return Err(status_error(response.status, &response.body));
        }
        let body: Value =
            serde_json::from_slice(&response.body).map_err(|err| AiError::Engine {
                reason: format!("the server's model list isn't JSON: {err}"),
            })?;
        Ok(body["data"]
            .as_array()
            .map(|models| {
                models
                    .iter()
                    .filter_map(|m| m["id"].as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default())
    }

    fn authorised(&self, request: Request) -> Request {
        match &self.api_key {
            Some(key) => request.header("Authorization", format!("Bearer {key}")),
            None => request,
        }
    }
}

/// The JSON body for `request`, streamed or not.
pub fn request_body(request: &ChatRequest, default_model: Option<&str>, stream: bool) -> Value {
    let mut body = json!({
        "messages": request.messages,
        "max_tokens": request.max_tokens,
        "temperature": request.temperature,
        "stream": stream,
    });
    if let Some(model) = request.model.as_deref().or(default_model) {
        body["model"] = json!(model);
    }
    body
}

/// One server-sent event's data, read.
#[derive(Debug, PartialEq, Eq)]
pub enum StreamEvent {
    /// More of the answer.
    Text(String),
    /// The answer ended; `reason` is the server's `finish_reason`.
    Finished { reason: String },
    /// `[DONE]`: the stream is over.
    Done,
    /// The server reported an error inside the stream.
    Error(String),
    /// Nothing for us (a role-only first chunk, a keep-alive).
    Nothing,
}

/// Reads one `data:` payload of a chat completions stream.
pub fn parse_event(data: &str) -> StreamEvent {
    let data = data.trim();
    if data == "[DONE]" {
        return StreamEvent::Done;
    }
    let Ok(value) = serde_json::from_str::<Value>(data) else {
        return StreamEvent::Nothing;
    };
    if let Some(error) = value.get("error") {
        let message = error["message"]
            .as_str()
            .map(str::to_string)
            .unwrap_or_else(|| error.to_string());
        return StreamEvent::Error(message);
    }
    let choice = &value["choices"][0];
    if let Some(text) = choice["delta"]["content"].as_str() {
        if !text.is_empty() {
            return StreamEvent::Text(text.to_string());
        }
    }
    if let Some(reason) = choice["finish_reason"].as_str() {
        return StreamEvent::Finished {
            reason: reason.to_string(),
        };
    }
    StreamEvent::Nothing
}

impl LanguageModel for OpenAiServer {
    fn describe(&self) -> ModelInfo {
        let id = self
            .model
            .clone()
            .unwrap_or_else(|| "server-default".into());
        ModelInfo {
            name: id.clone(),
            id,
            available: true,
            unavailable_reason: None,
            context_tokens: self.context_tokens,
        }
    }

    fn generate(&self, request: ChatRequest, out: Arc<dyn TextOut>) -> Result<(), AiError> {
        let body = request_body(&request, self.model.as_deref(), true);
        let http = self
            .authorised(Request::post(
                format!("{}/chat/completions", self.base_url),
                body.to_string(),
            ))
            .header("Content-Type", "application/json")
            .header("Accept", "text/event-stream")
            .timeout(FIRST_ANSWER_TIMEOUT);
        let cancel = CancelHandle::new();
        let response = self
            .client
            .open_stream_cancellable(&http, &cancel)
            .map_err(unreachable)?;
        if !response.is_success() {
            let mut body = Vec::new();
            let _ = std::io::Read::read_to_end(
                &mut std::io::Read::take(response.body, 16 * 1024),
                &mut body,
            );
            return Err(status_error(response.status, &body));
        }
        let reader = BufReader::new(response.body);
        for line in reader.lines() {
            if out.is_stopped() {
                cancel.cancel();
                return Ok(());
            }
            let line = match line {
                Ok(line) => line,
                Err(_) if out.is_stopped() => return Ok(()),
                Err(err) => {
                    return Err(AiError::Engine {
                        reason: format!("the server's answer broke off: {err}"),
                    })
                }
            };
            let Some(data) = line.strip_prefix("data:") else {
                continue;
            };
            match parse_event(data) {
                StreamEvent::Text(text) => {
                    if !out.push(&text) {
                        cancel.cancel();
                        return Ok(());
                    }
                }
                StreamEvent::Finished { reason } if reason == "content_filter" => {
                    return Err(AiError::Refused {
                        reason: "the server's model declined to answer this".into(),
                    })
                }
                StreamEvent::Done => return Ok(()),
                StreamEvent::Error(message) => return Err(AiError::Engine { reason: message }),
                StreamEvent::Finished { .. } | StreamEvent::Nothing => {}
            }
        }
        Ok(())
    }
}

fn unreachable(err: NetError) -> AiError {
    match err {
        NetError::Cancelled => AiError::Stopped,
        other => AiError::Unavailable {
            reason: format!("can't reach the model's server: {other}"),
        },
    }
}

fn status_error(status: u16, body: &[u8]) -> AiError {
    let text = String::from_utf8_lossy(body);
    let message = serde_json::from_str::<Value>(&text)
        .ok()
        .and_then(|v| v["error"]["message"].as_str().map(str::to_string))
        .unwrap_or_else(|| text.chars().take(200).collect());
    let lower = message.to_lowercase();
    if lower.contains("context") && (lower.contains("length") || lower.contains("exceed")) {
        return AiError::ContextTooLong { reason: message };
    }
    match status {
        404 => AiError::Unavailable {
            reason: format!("the server has no such model ({message})"),
        },
        429 | 503 => AiError::Busy { reason: message },
        _ => AiError::Engine {
            reason: format!("the server answered {status}: {message}"),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::Collect;
    use forge_net::{Method, Response, StaticClient};

    fn sse(chunks: &[&str]) -> String {
        let mut body = String::new();
        body.push_str("data: {\"choices\":[{\"delta\":{\"role\":\"assistant\"}}]}\n\n");
        for chunk in chunks {
            body.push_str(&format!(
                "data: {}\n\n",
                json!({"choices": [{"delta": {"content": chunk}}]})
            ));
        }
        body.push_str("data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n");
        body.push_str("data: [DONE]\n\n");
        body
    }

    #[test]
    fn the_body_is_a_chat_completions_request() {
        let request = ChatRequest::new("Be brief.", "Hello", 64).temperature(0.2);
        let body = request_body(&request, Some("qwen3:8b"), true);
        assert_eq!(body["model"], "qwen3:8b");
        assert_eq!(body["stream"], true);
        assert_eq!(body["max_tokens"], 64);
        assert_eq!(body["messages"][1]["role"], "user");
        let named = ChatRequest {
            model: Some("other".into()),
            ..request
        };
        assert_eq!(
            request_body(&named, Some("qwen3:8b"), false)["model"],
            "other"
        );
    }

    #[test]
    fn events_are_read() {
        assert_eq!(parse_event(" [DONE]"), StreamEvent::Done);
        assert_eq!(
            parse_event(r#"{"choices":[{"delta":{"content":"Hi"}}]}"#),
            StreamEvent::Text("Hi".into())
        );
        assert_eq!(
            parse_event(r#"{"choices":[{"delta":{},"finish_reason":"length"}]}"#),
            StreamEvent::Finished {
                reason: "length".into()
            }
        );
        assert_eq!(
            parse_event(r#"{"error":{"message":"model not found"}}"#),
            StreamEvent::Error("model not found".into())
        );
        assert_eq!(parse_event("not json"), StreamEvent::Nothing);
    }

    #[test]
    fn a_streamed_answer_arrives_in_pieces() {
        let client = Arc::new(StaticClient::new().route(
            Method::Post,
            "http://ai.test/v1/chat/completions",
            Ok(Response::new(200, sse(&["Hel", "lo", " there."]))),
        ));
        let server = OpenAiServer::new(client.clone(), "http://ai.test/v1/").model("tiny");
        let out = Arc::new(Collect::new());
        server
            .generate(ChatRequest::new("", "Hi", 20), out.clone())
            .unwrap();
        assert_eq!(out.text(), "Hello there.");
        assert_eq!(out.pieces(), 3);
        let sent: Value = serde_json::from_slice(&client.received()[0].body).unwrap();
        assert_eq!(sent["model"], "tiny");
    }

    #[test]
    fn stopping_ends_the_answer() {
        let client = Arc::new(StaticClient::new().route(
            Method::Post,
            "http://ai.test/v1/chat/completions",
            Ok(Response::new(200, sse(&["One ", "two ", "three"]))),
        ));
        let server = OpenAiServer::new(client, "http://ai.test/v1");
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = stop.clone();
        let out = Arc::new(
            Collect::new()
                .listen(move |_| flag.store(true, std::sync::atomic::Ordering::SeqCst))
                .stop_when(move || stop.load(std::sync::atomic::Ordering::SeqCst)),
        );
        server
            .generate(ChatRequest::new("", "Hi", 20), out.clone())
            .unwrap();
        assert_eq!(out.text(), "One ");
    }

    #[test]
    fn server_errors_are_read() {
        let client = Arc::new(StaticClient::new().route(
            Method::Post,
            "http://ai.test/v1/chat/completions",
            Ok(Response::new(
                404,
                r#"{"error":{"message":"model 'x' not found"}}"#,
            )),
        ));
        let server = OpenAiServer::new(client, "http://ai.test/v1");
        let err = server
            .generate(ChatRequest::new("", "Hi", 20), Arc::new(Collect::new()))
            .unwrap_err();
        assert!(matches!(err, AiError::Unavailable { .. }), "{err:?}");
        assert!(err.to_string().contains("model 'x' not found"));
    }

    #[test]
    fn the_model_list_is_read() {
        let client = Arc::new(StaticClient::new().route(
            Method::Get,
            "http://ai.test/v1/models",
            Ok(Response::new(
                200,
                r#"{"data":[{"id":"qwen3:8b"},{"id":"llama3.2"}]}"#,
            )),
        ));
        let server = OpenAiServer::new(client, "http://ai.test/v1");
        assert_eq!(server.models().unwrap(), vec!["qwen3:8b", "llama3.2"]);
    }
}
