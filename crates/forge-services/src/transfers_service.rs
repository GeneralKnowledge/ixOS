//! `native_services!(transfers)`: the UniFFI glue between an app's Rust
//! code and the host's background download session. The logic is in
//! [`crate::transfers`]; this is only the boundary.

#[cfg(feature = "transfers")]
#[doc(hidden)]
#[macro_export]
macro_rules! __native_transfers {
    () => {
        /// Background downloads that keep going while the app is suspended
        /// or ended by the system. iOS: a background `URLSession`. See
        /// `forge_services::transfers` and docs/native-services.md.
        pub mod transfers {
            use ::std::sync::{Arc, Weak};

            pub use $crate::transfers::{
                Failure, HostEvent, TransferError, TransferRecord, TransferRequest, TransferState,
                TransferUpdate, Transfers,
            };

            /// What the host is asked to download. The host names its task
            /// `id` (iOS: `taskDescription`) and, when the body has arrived,
            /// moves it to `<directory>/inbox/<id>` before its callback
            /// returns.
            #[derive(::uniffi::Record, Clone, Debug, PartialEq, Eq)]
            pub struct TransferTask {
                pub id: String,
                pub url: String,
                pub headers: Vec<TransferHeader>,
                pub expected_bytes: Option<u64>,
                pub allows_cellular: bool,
                pub discretionary: bool,
            }

            #[derive(::uniffi::Record, Clone, Debug, PartialEq, Eq)]
            pub struct TransferHeader {
                pub name: String,
                pub value: String,
            }

            /// A task the host's session still has after a launch.
            #[derive(::uniffi::Record, Clone, Debug, PartialEq, Eq)]
            pub struct SessionTaskInfo {
                pub id: String,
                pub received: u64,
                pub expected: Option<u64>,
                /// The host's own state for the task ("running",
                /// "suspended", ...), for the log only.
                pub state: String,
            }

            /// Where the host reports its tasks. Each call returns once Rust
            /// has saved the change (and verified and delivered a finished
            /// file), so the host may tell the system it is done afterwards.
            #[derive(::uniffi::Object)]
            pub struct TransferSink {
                core: Weak<Transfers>,
            }

            #[::uniffi::export]
            impl TransferSink {
                pub fn progress(&self, id: String, received: u64, expected: Option<u64>) {
                    self.handle(HostEvent::Progress {
                        id,
                        received,
                        expected,
                    });
                }

                /// The body is at `<directory>/inbox/<id>`. `http_status` is
                /// the response's status code.
                pub fn downloaded(&self, id: String, http_status: u16) {
                    self.handle(HostEvent::Downloaded {
                        id,
                        status: http_status,
                    });
                }

                /// The task ended without a body. `cancelled`: it was
                /// cancelled (by the app, or by the system on force quit).
                pub fn failed(&self, id: String, code: i64, reason: String, cancelled: bool) {
                    self.handle(HostEvent::Failed {
                        id,
                        code,
                        reason,
                        cancelled,
                    });
                }

                /// The session delivered every event of a background wake and
                /// the host is about to call the system's completion handler.
                pub fn events_handled(&self, session: String) {
                    $crate::__forge_runtime::logging::info!(
                        target: "transfers",
                        "background events handled for {session}; completing the wake"
                    );
                }

                /// The tasks the session has, after it is (re)created, and once more
                /// shortly after.
                pub fn reconcile(&self, tasks: Vec<SessionTaskInfo>) {
                    let states: Vec<&str> = tasks.iter().map(|t| t.state.as_str()).collect();
                    $crate::__forge_runtime::logging::info!(
                        target: "transfers",
                        "the session has {} tasks {:?}",
                        tasks.len(),
                        states
                    );
                    if let Some(core) = self.core.upgrade() {
                        let tasks: Vec<$crate::transfers::SessionTask> = tasks
                            .into_iter()
                            .map(|t| $crate::transfers::SessionTask {
                                id: t.id,
                                received: t.received,
                                expected: t.expected,
                            })
                            .collect();
                        core.reconcile(&tasks);
                    }
                }
            }

            impl TransferSink {
                fn handle(&self, event: HostEvent) {
                    if let Some(core) = self.core.upgrade() {
                        core.handle(event);
                    }
                }
            }

            /// Implemented by the host (iOS: a background `URLSession`).
            #[::uniffi::export(with_foreign)]
            pub trait TransferSession: Send + Sync {
                /// The folder this service keeps its ledger and inbox in
                /// (iOS: `Application Support/ForgeTransfers`).
                fn directory(&self) -> String;
                /// Creates the session (with the same identifier after a
                /// relaunch, so it picks up its tasks), sends every report to
                /// `sink` from now on, and calls `sink.reconcile` once with
                /// the tasks the session has.
                fn attach(&self, sink: Arc<TransferSink>);
                fn start(&self, task: TransferTask) -> Result<(), super::NativeServiceError>;
                fn cancel(&self, id: String);
            }

            struct HostSession(Arc<dyn TransferSession>);

            impl $crate::transfers::Session for HostSession {
                fn start(&self, request: &$crate::transfers::SessionRequest) -> Result<(), String> {
                    self.0
                        .start(TransferTask {
                            id: request.id.clone(),
                            url: request.url.clone(),
                            headers: request
                                .headers
                                .iter()
                                .map(|(name, value)| TransferHeader {
                                    name: name.clone(),
                                    value: value.clone(),
                                })
                                .collect(),
                            expected_bytes: request.expected_bytes,
                            allows_cellular: request.allows_cellular,
                            discretionary: request.discretionary,
                        })
                        .map_err(|err| err.to_string())
                }

                fn cancel(&self, id: &str) {
                    self.0.cancel(id.to_string());
                }
            }

            /// Called by the host at launch (`ForgeServices.install()`).
            #[::uniffi::export]
            pub fn install_native_transfers(
                session: Arc<dyn TransferSession>,
            ) -> Result<(), super::NativeServiceError> {
                let directory = session.directory();
                if directory.is_empty() {
                    return Err(super::NativeServiceError::InvalidInput {
                        reason: "the transfers directory is empty".into(),
                    });
                }
                let core = Transfers::open(directory, Arc::new(HostSession(session.clone())))
                    .map_err(|err| super::NativeServiceError::Unavailable {
                        reason: err.to_string(),
                    })?;
                let service = TransferService {
                    core: Arc::new(core),
                };
                let sink = Arc::new(TransferSink {
                    core: Arc::downgrade(&service.core),
                });
                service.install();
                session.attach(sink);
                Ok(())
            }

            /// Background downloads for Rust code. Cheap to clone; derefs to
            /// [`Transfers`] (`start`, `cancel`, `retry`, `list`,
            /// `deliver_to`, `watch`).
            #[derive(Clone)]
            pub struct TransferService {
                core: Arc<Transfers>,
            }

            $crate::__native_slot!(TransferService, "transfers");

            impl ::std::ops::Deref for TransferService {
                type Target = Transfers;
                fn deref(&self) -> &Transfers {
                    &self.core
                }
            }

            impl TransferService {
                /// A service over a fake session, keeping its ledger in
                /// `directory`. The test plays the host with the fake.
                pub fn for_tests(
                    directory: impl Into<::std::path::PathBuf>,
                ) -> (Self, Arc<$crate::transfers::testing::FakeSession>) {
                    let fake = Arc::new($crate::transfers::testing::FakeSession::default());
                    let core = Transfers::open(directory, fake.clone())
                        .expect("open a transfers ledger for tests");
                    (
                        Self {
                            core: Arc::new(core),
                        },
                        fake,
                    )
                }
            }
        }
    };
}

#[cfg(not(feature = "transfers"))]
#[doc(hidden)]
#[macro_export]
macro_rules! __native_transfers {
    () => {
        compile_error!(
            "native_services!(transfers) needs forge-services' `transfers` feature: \
             forge-services = { ..., features = [\"transfers\"] }"
        );
    };
}
