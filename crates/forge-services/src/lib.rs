//! Native platform services for Forge apps: secure storage (Keychain), app
//! directories and file import, local notifications, permissions, sharing
//! and deep links.
//!
//! Forge splits an app in two. Rust owns the logic; the native layer (Swift
//! on iOS) owns what makes the device a device. This crate is the line
//! between them: a few small Rust interfaces that the host implements once,
//! with ready-made Swift implementations that Forge ships
//! (`crates/forge-services/swift/`, copied into the app's `Generated/ForgeFFI`
//! by `forge build`).
//!
//! # Using it
//!
//! In the app crate (`rust/src/lib.rs`), after `uniffi::setup_scaffolding!`:
//!
//! ```ignore
//! forge_services::native_services!();
//! ```
//!
//! That defines a `native` module in the app crate. Its UniFFI items join the
//! app's own `forge_ffi` namespace, so they appear in the same Swift module
//! (`ForgeFFI`) and Kotlin package as the app's API, with no extra build step.
//! The iOS host calls `ForgeServices.install()` once at launch; Rust then uses
//! `native::NativeServices::installed()`:
//!
//! ```ignore
//! let services = native::NativeServices::installed()?;
//! services.write_secret_string("api-token", &token)?;
//! services.schedule_notification(native::LocalNotification::now("id", "Title", "Body")).await?;
//! ```
//!
//! See `docs/native-services.md` for the whole picture and how to add a
//! capability.
//!
//! # Why a macro
//!
//! UniFFI gives each crate its own namespace, which would mean a second Swift
//! module and a second C header per app. Emitting the items from the app
//! crate keeps one namespace. The logic that does not cross the boundary
//! (URL parsing, validation) lives here as plain Rust and is tested here.

mod feed;
pub mod link;
pub mod notify;
mod optional;
pub mod sensors;
mod slot;

pub use feed::{Feed, Watch};
pub use slot::Slot;

/// Version of the interface between the Rust items and the Swift adapters.
/// The adapters check it at install time; bump it whenever a trait, record
/// or enum in [`native_services!`] changes shape.
pub const VERSION: u32 = 2;

/// Longest secure-store key accepted.
pub const MAX_SECRET_KEY_LEN: usize = 256;

/// Checks a secure-store key: non-empty, at most [`MAX_SECRET_KEY_LEN`]
/// bytes, no control characters.
pub fn validate_secret_key(key: &str) -> Result<(), String> {
    if key.is_empty() {
        return Err("a secure-store key must not be empty".into());
    }
    if key.len() > MAX_SECRET_KEY_LEN {
        return Err(format!(
            "a secure-store key is at most {MAX_SECRET_KEY_LEN} bytes"
        ));
    }
    if key.chars().any(char::is_control) {
        return Err("a secure-store key must not contain control characters".into());
    }
    Ok(())
}

/// Checks that the host gave absolute directory paths.
pub fn validate_directory(name: &str, path: &str) -> Result<(), String> {
    if path.is_empty() {
        return Err(format!("the {name} directory is empty"));
    }
    if !std::path::Path::new(path).is_absolute() {
        return Err(format!(
            "the {name} directory must be absolute, got {path:?}"
        ));
    }
    Ok(())
}

/// Defines the `native` module in the calling crate: the records, errors and
/// traits of the native services layer, as UniFFI items of the calling
/// crate's namespace, plus the Rust-side API (`native::NativeServices`).
///
/// The calling crate must depend on `uniffi` and `async-trait` directly
/// (the `forge init` template does). Call it once, at the crate root.
///
/// Optional services are named in the call and become submodules of
/// `native`; only their Swift adapters are copied into the app:
///
/// ```ignore
/// forge_services::native_services!(connectivity, device_status, motion, location, bluetooth);
/// // native::connectivity::ConnectivityService::installed()?.current()
/// ```
#[macro_export]
macro_rules! native_services {
    ($($service:ident),* $(,)?) => {
        /// Native platform services (Keychain, directories, notifications,
        /// permissions, deep links), implemented by the host app.
        ///
        /// The iOS host installs them with `ForgeServices.install()` at
        /// launch. Rust code gets them with [`NativeServices::installed`];
        /// tests use [`NativeServices::for_tests`].
        pub mod native {
            use ::std::sync::Arc;

            // ---------------------------------------------------------- types

            /// Where the app may keep files. Absolute paths, given by the host
            /// at launch. On iOS the container path changes between installs,
            /// so store paths relative to one of these, never absolute ones.
            #[derive(::uniffi::Record, Clone, Debug, PartialEq, Eq)]
            pub struct AppDirectories {
                /// User-visible documents (shown in the Files app when the app
                /// opts in). Backed up.
                pub documents: String,
                /// The app's own data. Backed up, never shown to the user.
                pub application_support: String,
                /// Re-creatable data. The system may delete it when space is low.
                pub caches: String,
                /// Scratch space, cleared by the system between launches.
                pub temporary: String,
            }

            /// Errors from the native services. `reason` fields are
            /// diagnostics, not UI copy.
            #[derive(::uniffi::Error, Clone, Debug, PartialEq, Eq)]
            pub enum NativeServiceError {
                /// The host has not called `install_native_services` (on iOS,
                /// `ForgeServices.install()`), or this platform has no
                /// implementation yet.
                NotInstalled { service: String },
                /// The arguments were rejected before reaching the platform.
                InvalidInput { reason: String },
                /// The user has not allowed this (for example notifications
                /// are turned off for the app).
                PermissionDenied { reason: String },
                /// The service cannot work in this process (for example no
                /// notification centre under `swift test`).
                Unavailable { reason: String },
                /// The platform reported an error. On iOS `code` is the
                /// `OSStatus` or `NSError` code.
                Platform { code: i64, reason: String },
            }

            impl ::std::fmt::Display for NativeServiceError {
                fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                    match self {
                        Self::NotInstalled { service } => {
                            write!(f, "native service not installed: {service}")
                        }
                        Self::InvalidInput { reason } => write!(f, "invalid input: {reason}"),
                        Self::PermissionDenied { reason } => {
                            write!(f, "permission denied: {reason}")
                        }
                        Self::Unavailable { reason } => write!(f, "unavailable: {reason}"),
                        Self::Platform { code, reason } => {
                            write!(f, "platform error {code}: {reason}")
                        }
                    }
                }
            }

            impl ::std::error::Error for NativeServiceError {}

            impl ::std::convert::From<::uniffi::UnexpectedUniFFICallbackError> for NativeServiceError {
                fn from(err: ::uniffi::UnexpectedUniFFICallbackError) -> Self {
                    Self::Platform {
                        code: -1,
                        reason: err.reason,
                    }
                }
            }

            /// A permission the app can ask the user for. More cases (camera,
            /// photos, microphone) are added as their adapters are; see
            /// docs/native-services.md. A permission whose service the app
            /// did not select reports `Unsupported`.
            #[derive(::uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq, Hash)]
            pub enum NativePermission {
                /// Alerts, sounds and badges from local notifications.
                Notifications,
                /// Location while the app is in use (`location` service).
                Location,
                /// Motion & Fitness: the barometer, pedometer and activity
                /// (`motion` service). The other motion sensors need none.
                Motion,
                /// Bluetooth (`bluetooth` service).
                Bluetooth,
            }

            /// The same states on every platform.
            #[derive(::uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq, Hash)]
            pub enum PermissionStatus {
                /// Never asked. A request shows the system prompt.
                NotDetermined,
                Granted,
                /// iOS provisional notifications: delivered quietly, without
                /// asking first.
                Provisional,
                /// The user said no. Only the Settings app can change it.
                Denied,
                /// Blocked by parental controls or device management.
                Restricted,
                /// This platform or process has no such permission.
                Unsupported,
            }

            impl PermissionStatus {
                /// True when the capability may be used now.
                pub fn allows_use(self) -> bool {
                    matches!(self, Self::Granted | Self::Provisional)
                }
            }

            /// When a local notification fires.
            #[derive(::uniffi::Enum, Clone, Debug, PartialEq, Eq)]
            pub enum NotificationTrigger {
                /// Straight away (shown as a banner even while the app is open).
                Now,
                /// After `seconds`. Repeating needs at least 60 seconds on iOS.
                After { seconds: u32, repeats: bool },
                /// At a moment in time (Unix milliseconds), in the device's
                /// time zone at that moment. Must be in the future.
                At { unix_ms: i64 },
            }

            /// A local notification. Scheduling one with an `id` that is
            /// already pending replaces it.
            #[derive(::uniffi::Record, Clone, Debug, PartialEq, Eq)]
            pub struct LocalNotification {
                pub id: String,
                pub title: String,
                pub body: String,
                pub subtitle: Option<String>,
                /// Groups related notifications (iOS `threadIdentifier`).
                pub thread_id: Option<String>,
                /// Opened when the user taps the notification, through the
                /// same path as any other deep link.
                pub deep_link: Option<String>,
                pub trigger: NotificationTrigger,
            }

            impl LocalNotification {
                /// A notification shown straight away.
                pub fn now(
                    id: impl Into<String>,
                    title: impl Into<String>,
                    body: impl Into<String>,
                ) -> Self {
                    Self {
                        id: id.into(),
                        title: title.into(),
                        body: body.into(),
                        subtitle: None,
                        thread_id: None,
                        deep_link: None,
                        trigger: NotificationTrigger::Now,
                    }
                }
            }

            /// One `name=value` pair of a URL query, decoded.
            #[derive(::uniffi::Record, Clone, Debug, PartialEq, Eq)]
            pub struct QueryItem {
                pub name: String,
                pub value: String,
            }

            /// A URL the app was opened with (custom scheme, universal link
            /// or notification tap), split into parts.
            #[derive(::uniffi::Record, Clone, Debug, PartialEq, Eq)]
            pub struct DeepLink {
                /// The URL as received.
                pub url: String,
                /// Lowercased, e.g. `myapp` or `https`.
                pub scheme: String,
                /// Lowercased host, if the URL has one.
                pub host: Option<String>,
                /// Decoded path segments, empty ones dropped.
                pub path: Vec<String>,
                pub query: Vec<QueryItem>,
                pub fragment: Option<String>,
                /// What to route on. For `http(s)` links the path; for custom
                /// schemes the host followed by the path, so
                /// `myapp://note/42` and `https://example.com/note/42` both
                /// give `["note", "42"]`.
                pub route: Vec<String>,
            }

            impl DeepLink {
                /// The first value of query parameter `name`.
                pub fn query_value(&self, name: &str) -> Option<&str> {
                    self.query
                        .iter()
                        .find(|item| item.name == name)
                        .map(|item| item.value.as_str())
                }
            }

            /// Something the user can share through the system share sheet.
            /// The host presents it (on iOS `ForgeShareLink`).
            #[derive(::uniffi::Enum, Clone, Debug, PartialEq, Eq)]
            pub enum ShareItem {
                Text { text: String },
                Url { url: String },
                /// A file the app wrote, by absolute path.
                File { path: String, title: Option<String> },
            }

            // ---------------------------------------------- host interfaces

            /// Secrets (tokens, keys, passwords), stored by the platform's
            /// secure store: the iOS Keychain, readable after the first
            /// unlock and never synced or restored to another device.
            /// Survives app updates; on iOS it can survive deleting the app.
            #[::uniffi::export(with_foreign)]
            pub trait SecureStore: Send + Sync {
                /// The value stored under `key`, or `None`.
                fn get(&self, key: String) -> Result<Option<Vec<u8>>, NativeServiceError>;
                /// Stores `value` under `key`, replacing any old value.
                fn set(&self, key: String, value: Vec<u8>) -> Result<(), NativeServiceError>;
                /// Removes `key`. Removing a missing key is not an error.
                fn delete(&self, key: String) -> Result<(), NativeServiceError>;
            }

            /// Local notifications (no push server involved).
            #[::uniffi::export(with_foreign)]
            #[::async_trait::async_trait]
            pub trait LocalNotifier: Send + Sync {
                /// Schedules (or replaces) a notification. Fails with
                /// `PermissionDenied` when notifications are off for the app.
                async fn schedule(
                    &self,
                    notification: LocalNotification,
                ) -> Result<(), NativeServiceError>;
                /// Cancels a pending notification and removes it from the
                /// notification list if it was delivered.
                fn cancel(&self, id: String);
                /// Ids of the notifications that are scheduled and not yet
                /// delivered.
                async fn pending(&self) -> Vec<String>;
            }

            /// Permission status and requests, one interface for every
            /// permission.
            #[::uniffi::export(with_foreign)]
            #[::async_trait::async_trait]
            pub trait PermissionProvider: Send + Sync {
                async fn status(&self, permission: NativePermission) -> PermissionStatus;
                /// Shows the system prompt if the user has not been asked
                /// yet; otherwise returns the current status. Call it in
                /// response to something the user did.
                async fn request(&self, permission: NativePermission) -> PermissionStatus;
            }

            // ------------------------------------------------------- exports

            /// The services interface version these bindings were built
            /// with. The host's adapters check it at install time.
            #[::uniffi::export]
            pub fn native_services_version() -> u32 {
                $crate::VERSION
            }

            /// True once the host has installed the services. For
            /// diagnostics and tests.
            #[::uniffi::export]
            pub fn native_services_installed() -> bool {
                INSTALLED.get().is_some()
            }

            /// Called once by the host at launch (on iOS by
            /// `ForgeServices.install()`). Installing again replaces the
            /// previous services.
            #[::uniffi::export]
            pub fn install_native_services(
                directories: AppDirectories,
                secure_store: Arc<dyn SecureStore>,
                notifier: Arc<dyn LocalNotifier>,
                permissions: Arc<dyn PermissionProvider>,
            ) -> Result<(), NativeServiceError> {
                NativeServices {
                    directories,
                    secure_store,
                    notifier,
                    permissions,
                }
                .install()
            }

            /// Splits a URL the app was opened with into its parts.
            #[::uniffi::export]
            pub fn parse_deep_link(url: String) -> Result<DeepLink, NativeServiceError> {
                let parsed = $crate::link::parse(&url)
                    .map_err(|reason| NativeServiceError::InvalidInput { reason })?;
                Ok(DeepLink {
                    url,
                    scheme: parsed.scheme,
                    host: parsed.host,
                    path: parsed.path,
                    query: parsed
                        .query
                        .into_iter()
                        .map(|(name, value)| QueryItem { name, value })
                        .collect(),
                    fragment: parsed.fragment,
                    route: parsed.route,
                })
            }

            // ------------------------------------------------- Rust-side API

            static INSTALLED: $crate::Slot<NativeServices> = $crate::Slot::new();

            /// The installed services. Cheap to clone; take one when your
            /// app object is created and keep it, or pass one in from tests.
            #[derive(Clone)]
            pub struct NativeServices {
                pub directories: AppDirectories,
                pub secure_store: Arc<dyn SecureStore>,
                pub notifier: Arc<dyn LocalNotifier>,
                pub permissions: Arc<dyn PermissionProvider>,
            }

            impl ::std::fmt::Debug for NativeServices {
                fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                    f.debug_struct("NativeServices")
                        .field("directories", &self.directories)
                        .finish_non_exhaustive()
                }
            }

            impl NativeServices {
                /// The services the host installed, or `NotInstalled`.
                pub fn installed() -> Result<Self, NativeServiceError> {
                    INSTALLED.get().ok_or_else(|| NativeServiceError::NotInstalled {
                        service: "native services (call ForgeServices.install() at launch)".into(),
                    })
                }

                /// Makes these the installed services (checks the directories).
                pub fn install(self) -> Result<(), NativeServiceError> {
                    let dirs = &self.directories;
                    for (name, path) in [
                        ("documents", &dirs.documents),
                        ("application support", &dirs.application_support),
                        ("caches", &dirs.caches),
                        ("temporary", &dirs.temporary),
                    ] {
                        $crate::validate_directory(name, path)
                            .map_err(|reason| NativeServiceError::InvalidInput { reason })?;
                    }
                    INSTALLED.set(self);
                    Ok(())
                }

                /// Removes the installed services (for tests).
                pub fn uninstall() {
                    INSTALLED.clear();
                }

                /// In-memory services under `root` (directories created if
                /// missing), for tests on any machine, with the fakes behind
                /// them so a test can inspect or steer them.
                pub fn for_tests(root: impl AsRef<::std::path::Path>) -> (Self, testing::Fakes) {
                    let root = root.as_ref();
                    let dir = |name: &str| {
                        let path = root.join(name);
                        let _ = ::std::fs::create_dir_all(&path);
                        path.to_string_lossy().into_owned()
                    };
                    let fakes = testing::Fakes::default();
                    let services = Self {
                        directories: AppDirectories {
                            documents: dir("Documents"),
                            application_support: dir("Application Support"),
                            caches: dir("Caches"),
                            temporary: dir("tmp"),
                        },
                        secure_store: fakes.secure_store.clone(),
                        notifier: fakes.notifier.clone(),
                        permissions: fakes.permissions.clone(),
                    };
                    (services, fakes)
                }

                /// Reads a UTF-8 secret.
                pub fn read_secret_string(
                    &self,
                    key: &str,
                ) -> Result<Option<String>, NativeServiceError> {
                    check_key(key)?;
                    match self.secure_store.get(key.to_string())? {
                        None => Ok(None),
                        Some(bytes) => String::from_utf8(bytes).map(Some).map_err(|_| {
                            NativeServiceError::InvalidInput {
                                reason: format!("the secret {key:?} is not UTF-8"),
                            }
                        }),
                    }
                }

                /// Stores a UTF-8 secret.
                pub fn write_secret_string(
                    &self,
                    key: &str,
                    value: &str,
                ) -> Result<(), NativeServiceError> {
                    check_key(key)?;
                    self.secure_store
                        .set(key.to_string(), value.as_bytes().to_vec())
                }

                /// Removes a secret.
                pub fn delete_secret(&self, key: &str) -> Result<(), NativeServiceError> {
                    check_key(key)?;
                    self.secure_store.delete(key.to_string())
                }

                /// Checks `notification` and schedules it.
                pub async fn schedule_notification(
                    &self,
                    notification: LocalNotification,
                ) -> Result<(), NativeServiceError> {
                    let trigger = match &notification.trigger {
                        NotificationTrigger::Now => $crate::notify::Trigger::Now,
                        NotificationTrigger::After { seconds, repeats } => {
                            $crate::notify::Trigger::After {
                                seconds: *seconds,
                                repeats: *repeats,
                            }
                        }
                        NotificationTrigger::At { unix_ms } => {
                            $crate::notify::Trigger::At { unix_ms: *unix_ms }
                        }
                    };
                    $crate::notify::validate(
                        &notification.id,
                        &notification.title,
                        &notification.body,
                        notification.deep_link.as_deref(),
                        trigger,
                        $crate::notify::now_ms(),
                    )
                    .map_err(|reason| NativeServiceError::InvalidInput { reason })?;
                    self.notifier.schedule(notification).await
                }

                pub fn cancel_notification(&self, id: &str) {
                    self.notifier.cancel(id.to_string());
                }

                pub async fn permission_status(&self, permission: NativePermission) -> PermissionStatus {
                    self.permissions.status(permission).await
                }

                pub async fn request_permission(&self, permission: NativePermission) -> PermissionStatus {
                    self.permissions.request(permission).await
                }
            }

            fn check_key(key: &str) -> Result<(), NativeServiceError> {
                $crate::validate_secret_key(key)
                    .map_err(|reason| NativeServiceError::InvalidInput { reason })
            }

            /// In-memory implementations of the host interfaces, for Rust
            /// tests (and for hosts without a native implementation yet).
            pub mod testing {
                use super::*;
                use ::std::collections::HashMap;
                use ::std::sync::Mutex;

                /// The fakes behind [`NativeServices::for_tests`].
                #[derive(Default, Clone)]
                pub struct Fakes {
                    pub secure_store: Arc<MemorySecureStore>,
                    pub notifier: Arc<RecordingNotifier>,
                    pub permissions: Arc<FixedPermissions>,
                }

                fn lock<T>(mutex: &Mutex<T>) -> ::std::sync::MutexGuard<'_, T> {
                    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
                }

                /// A secure store in a `HashMap`.
                #[derive(Default)]
                pub struct MemorySecureStore {
                    values: Mutex<HashMap<String, Vec<u8>>>,
                }

                impl SecureStore for MemorySecureStore {
                    fn get(&self, key: String) -> Result<Option<Vec<u8>>, NativeServiceError> {
                        Ok(lock(&self.values).get(&key).cloned())
                    }
                    fn set(&self, key: String, value: Vec<u8>) -> Result<(), NativeServiceError> {
                        lock(&self.values).insert(key, value);
                        Ok(())
                    }
                    fn delete(&self, key: String) -> Result<(), NativeServiceError> {
                        lock(&self.values).remove(&key);
                        Ok(())
                    }
                }

                /// Keeps scheduled notifications in memory. Refuses them with
                /// `PermissionDenied` after [`RecordingNotifier::deny`].
                #[derive(Default)]
                pub struct RecordingNotifier {
                    pending: Mutex<Vec<LocalNotification>>,
                    denied: ::std::sync::atomic::AtomicBool,
                }

                impl RecordingNotifier {
                    pub fn deny(&self) {
                        self.denied.store(true, ::std::sync::atomic::Ordering::SeqCst);
                    }
                    /// Everything scheduled and not cancelled, oldest first.
                    pub fn scheduled(&self) -> Vec<LocalNotification> {
                        lock(&self.pending).clone()
                    }
                }

                #[::async_trait::async_trait]
                impl LocalNotifier for RecordingNotifier {
                    async fn schedule(
                        &self,
                        notification: LocalNotification,
                    ) -> Result<(), NativeServiceError> {
                        if self.denied.load(::std::sync::atomic::Ordering::SeqCst) {
                            return Err(NativeServiceError::PermissionDenied {
                                reason: "notifications are turned off".into(),
                            });
                        }
                        let mut pending = lock(&self.pending);
                        pending.retain(|n| n.id != notification.id);
                        pending.push(notification);
                        Ok(())
                    }
                    fn cancel(&self, id: String) {
                        lock(&self.pending).retain(|n| n.id != id);
                    }
                    async fn pending(&self) -> Vec<String> {
                        lock(&self.pending).iter().map(|n| n.id.clone()).collect()
                    }
                }

                /// Permissions with fixed answers: `NotDetermined` until set,
                /// and a request grants anything not set to something else.
                #[derive(Default)]
                pub struct FixedPermissions {
                    statuses: Mutex<HashMap<NativePermission, PermissionStatus>>,
                }

                impl FixedPermissions {
                    pub fn set(&self, permission: NativePermission, status: PermissionStatus) {
                        lock(&self.statuses).insert(permission, status);
                    }
                }

                #[::async_trait::async_trait]
                impl PermissionProvider for FixedPermissions {
                    async fn status(&self, permission: NativePermission) -> PermissionStatus {
                        lock(&self.statuses)
                            .get(&permission)
                            .copied()
                            .unwrap_or(PermissionStatus::NotDetermined)
                    }
                    async fn request(&self, permission: NativePermission) -> PermissionStatus {
                        *lock(&self.statuses)
                            .entry(permission)
                            .and_modify(|status| {
                                if *status == PermissionStatus::NotDetermined {
                                    *status = PermissionStatus::Granted;
                                }
                            })
                            .or_insert(PermissionStatus::Granted)
                    }
                }
            }

            $( $crate::__native_service!($service); )*
        }
    };
}
