//! A language model that answers from a script, for tests on Linux.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::{AiError, ChatRequest, LanguageModel, ModelInfo, TextOut};

/// Answers each request with the next scripted reply, word by word (the
/// last reply repeats once the script runs out), and records every request.
pub struct ScriptedModel {
    replies: Mutex<VecDeque<Result<String, AiError>>>,
    last: Mutex<Result<String, AiError>>,
    requests: Mutex<Vec<ChatRequest>>,
    info: ModelInfo,
    pause: Duration,
}

impl ScriptedModel {
    pub fn new<S: Into<String>>(replies: impl IntoIterator<Item = S>) -> Self {
        Self::with_results(replies.into_iter().map(|r| Ok(r.into())))
    }

    /// Replies that may fail.
    pub fn with_results(replies: impl IntoIterator<Item = Result<String, AiError>>) -> Self {
        Self {
            replies: Mutex::new(replies.into_iter().collect()),
            last: Mutex::new(Ok(String::new())),
            requests: Mutex::new(Vec::new()),
            info: ModelInfo {
                id: "scripted".into(),
                name: "Scripted model".into(),
                available: true,
                unavailable_reason: None,
                context_tokens: 4096,
            },
            pause: Duration::ZERO,
        }
    }

    /// Describes itself as `info`.
    pub fn info(mut self, info: ModelInfo) -> Self {
        self.info = info;
        self
    }

    /// Waits this long before each word, so a test can stop it midway.
    pub fn pause(mut self, pause: Duration) -> Self {
        self.pause = pause;
        self
    }

    /// The requests it was asked, oldest first.
    pub fn requests(&self) -> Vec<ChatRequest> {
        self.requests
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }
}

impl LanguageModel for ScriptedModel {
    fn describe(&self) -> ModelInfo {
        self.info.clone()
    }

    fn generate(&self, request: ChatRequest, out: Arc<dyn TextOut>) -> Result<(), AiError> {
        self.requests
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push(request);
        let reply = {
            let mut last = self.last.lock().unwrap_or_else(|p| p.into_inner());
            if let Some(next) = self
                .replies
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .pop_front()
            {
                *last = next;
            }
            last.clone()
        }?;
        for (i, word) in reply.split_inclusive(' ').enumerate() {
            if i > 0 && !self.pause.is_zero() {
                std::thread::sleep(self.pause);
            }
            if out.is_stopped() || !out.push(word) {
                return Ok(());
            }
        }
        Ok(())
    }
}
