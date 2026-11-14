//! `native_services!(ai)`: the UniFFI glue between Forge AI
//! ([`crate::ai`]) and the host's on-device model (iOS: Apple's Foundation
//! Models, `ForgeServices+Ai.swift`). The logic is in [`crate::ai`].

#[cfg(feature = "ai")]
#[doc(hidden)]
#[macro_export]
macro_rules! __native_ai {
    () => {
        /// A language model on the device (iOS 26: Apple Intelligence through
        /// Foundation Models). See `forge_services::ai` and
        /// docs/forge-ai-plan.md.
        pub mod ai {
            use ::std::sync::Arc;

            pub use $crate::ai::{
                ask, AiError, ChatMessage, ChatRequest, Collect, LanguageModel, ModelInfo, Role,
                ScriptedModel, TextOut,
            };

            /// Who wrote a message.
            #[derive(::uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq, Hash)]
            pub enum AiRole {
                System,
                User,
                Assistant,
            }

            #[derive(::uniffi::Record, Clone, Debug, PartialEq, Eq)]
            pub struct AiMessage {
                pub role: AiRole,
                pub content: String,
            }

            /// One request: OpenAI's chat completions, text only. System
            /// messages come first; the last message is the one to answer.
            #[derive(::uniffi::Record, Clone, Debug, PartialEq)]
            pub struct AiRequest {
                pub model: Option<String>,
                pub messages: Vec<AiMessage>,
                /// The longest answer, in tokens.
                pub max_tokens: u32,
                /// Sampling temperature, 0 to 1.5.
                pub temperature: f64,
            }

            /// What a model is and whether it can answer now.
            #[derive(::uniffi::Record, Clone, Debug, PartialEq, Eq)]
            pub struct AiModelInfo {
                /// Stable id, recorded with each answer
                /// ("apple-foundation-models").
                pub id: String,
                /// The name the app shows ("Apple Intelligence").
                pub name: String,
                pub available: bool,
                /// Why not, in words the user can act on.
                pub unavailable_reason: Option<String>,
                /// Tokens one request can read and write together.
                pub context_tokens: u32,
            }

            /// Why the model gave no answer.
            #[derive(::uniffi::Error, Clone, Debug, PartialEq, Eq)]
            pub enum AiModelError {
                /// Apple Intelligence off or still downloading, an iPhone
                /// that doesn't support it.
                Unavailable {
                    reason: String,
                },
                /// The model's safety rules declined the request.
                Refused {
                    reason: String,
                },
                /// The request doesn't fit the model's context window.
                ContextTooLong {
                    reason: String,
                },
                /// Throttled for now (Apple's model limits apps in the
                /// background).
                Busy {
                    reason: String,
                },
                /// Stopped through the sink.
                Stopped,
                Engine {
                    reason: String,
                },
            }

            impl ::std::fmt::Display for AiModelError {
                fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                    ::std::fmt::Display::fmt(&AiError::from(self.clone()), f)
                }
            }

            impl ::std::error::Error for AiModelError {}

            impl ::std::convert::From<::uniffi::UnexpectedUniFFICallbackError> for AiModelError {
                fn from(err: ::uniffi::UnexpectedUniFFICallbackError) -> Self {
                    Self::Engine { reason: err.reason }
                }
            }

            impl ::std::convert::From<AiModelError> for AiError {
                fn from(err: AiModelError) -> Self {
                    match err {
                        AiModelError::Unavailable { reason } => AiError::Unavailable { reason },
                        AiModelError::Refused { reason } => AiError::Refused { reason },
                        AiModelError::ContextTooLong { reason } => {
                            AiError::ContextTooLong { reason }
                        }
                        AiModelError::Busy { reason } => AiError::Busy { reason },
                        AiModelError::Stopped => AiError::Stopped,
                        AiModelError::Engine { reason } => AiError::Engine { reason },
                    }
                }
            }

            impl ::std::convert::From<AiError> for AiModelError {
                fn from(err: AiError) -> Self {
                    match err {
                        AiError::Unavailable { reason } => Self::Unavailable { reason },
                        AiError::Refused { reason } => Self::Refused { reason },
                        AiError::ContextTooLong { reason } => Self::ContextTooLong { reason },
                        AiError::Busy { reason } => Self::Busy { reason },
                        AiError::Stopped => Self::Stopped,
                        AiError::Engine { reason } => Self::Engine { reason },
                    }
                }
            }

            impl ::std::convert::From<ChatRequest> for AiRequest {
                fn from(request: ChatRequest) -> Self {
                    Self {
                        model: request.model,
                        messages: request
                            .messages
                            .into_iter()
                            .map(|m| AiMessage {
                                role: match m.role {
                                    Role::System => AiRole::System,
                                    Role::User => AiRole::User,
                                    Role::Assistant => AiRole::Assistant,
                                },
                                content: m.content,
                            })
                            .collect(),
                        max_tokens: request.max_tokens,
                        temperature: request.temperature,
                    }
                }
            }

            impl ::std::convert::From<AiRequest> for ChatRequest {
                fn from(request: AiRequest) -> Self {
                    Self {
                        model: request.model,
                        messages: request
                            .messages
                            .into_iter()
                            .map(|m| ChatMessage {
                                role: match m.role {
                                    AiRole::System => Role::System,
                                    AiRole::User => Role::User,
                                    AiRole::Assistant => Role::Assistant,
                                },
                                content: m.content,
                            })
                            .collect(),
                        max_tokens: request.max_tokens,
                        temperature: request.temperature,
                    }
                }
            }

            impl ::std::convert::From<ModelInfo> for AiModelInfo {
                fn from(info: ModelInfo) -> Self {
                    Self {
                        id: info.id,
                        name: info.name,
                        available: info.available,
                        unavailable_reason: info.unavailable_reason,
                        context_tokens: info.context_tokens,
                    }
                }
            }

            impl ::std::convert::From<AiModelInfo> for ModelInfo {
                fn from(info: AiModelInfo) -> Self {
                    Self {
                        id: info.id,
                        name: info.name,
                        available: info.available,
                        unavailable_reason: info.unavailable_reason,
                        context_tokens: info.context_tokens,
                    }
                }
            }

            /// Where the model writes its answer. Rust decides what each
            /// piece does (show it, keep it, stop the model).
            #[derive(::uniffi::Object)]
            pub struct TextSink {
                out: Arc<dyn TextOut>,
            }

            impl TextSink {
                /// A sink that passes each piece to `out`.
                pub fn new(out: Arc<dyn TextOut>) -> Arc<Self> {
                    Arc::new(Self { out })
                }
            }

            #[::uniffi::export]
            impl TextSink {
                /// Adds the next piece of the answer (only the new text).
                /// Returns false once the answer was stopped: the model must
                /// stop computing. Cheap; push every piece as it comes.
                pub fn push(&self, text: String) -> bool {
                    self.out.push(&text)
                }

                /// Whether the answer was stopped, for a model that can
                /// check between steps without pushing.
                pub fn is_stopped(&self) -> bool {
                    self.out.is_stopped()
                }
            }

            /// A model on the device, implemented by the host (iOS:
            /// `ForgeAppleModel`). Rust calls `generate` from a thread of
            /// its own, one request at a time.
            #[::uniffi::export(with_foreign)]
            #[::async_trait::async_trait]
            pub trait AiModel: Send + Sync {
                /// Which model this is and whether it can answer now. Cheap.
                fn describe(&self) -> AiModelInfo;

                /// Writes the answer to `request` into `sink`, piece by
                /// piece, and returns when it is complete, or as soon as
                /// `sink.push` returns false.
                async fn generate(
                    &self,
                    request: AiRequest,
                    sink: Arc<TextSink>,
                ) -> Result<(), AiModelError>;
            }

            /// The host's model as a [`LanguageModel`]: `generate` blocks
            /// its thread until the host is done.
            #[derive(Clone)]
            pub struct HostModel(pub Arc<dyn AiModel>);

            impl LanguageModel for HostModel {
                fn describe(&self) -> ModelInfo {
                    self.0.describe().into()
                }

                fn generate(
                    &self,
                    request: ChatRequest,
                    out: Arc<dyn TextOut>,
                ) -> Result<(), AiError> {
                    $crate::ai::block_on(self.0.generate(request.into(), TextSink::new(out)))
                        .map_err(AiError::from)
                }
            }

            /// Called by the host at launch (`ForgeServices.install()`):
            /// makes `model` the device's model.
            #[::uniffi::export]
            pub fn install_native_ai(model: Arc<dyn AiModel>) {
                AiService(HostModel(model)).install();
            }

            /// The device's model, once the host installed it.
            #[derive(Clone)]
            pub struct AiService(pub HostModel);

            $crate::__native_slot!(AiService, "ai");

            impl AiService {
                /// The installed model as a [`LanguageModel`].
                pub fn model(&self) -> Arc<dyn LanguageModel> {
                    Arc::new(self.0.clone())
                }
            }
        }
    };
}

#[cfg(not(feature = "ai"))]
#[doc(hidden)]
#[macro_export]
macro_rules! __native_ai {
    () => {
        compile_error!(
            "native_services!(ai) needs forge-services' `ai` feature: \
             forge-services = { ..., features = [\"ai\"] }"
        );
    };
}
