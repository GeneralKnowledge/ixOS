//! The `biometrics` optional service: Face ID, Touch ID or the device
//! passcode, through LocalAuthentication. Optional because it needs
//! `[ios.permissions] face_id` (NSFaceIDUsageDescription): iOS ends an app
//! that asks for Face ID without it.
//!
//! The jobs apps repeat are built in: say what the device offers (for a
//! "Require Face ID" setting), confirm the person before a sensitive action,
//! and lock the app (the SwiftUI `.forgeAppLock` modifier in the adapter).

#[doc(hidden)]
#[macro_export]
macro_rules! __native_biometrics {
    () => {
        /// Face ID, Touch ID or the device passcode. iOS: LocalAuthentication.
        /// Needs `[ios.permissions] face_id` in forge.toml.
        pub mod biometrics {
            use ::std::sync::Arc;

            /// The biometric sensor the device has.
            #[derive(::uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
            pub enum BiometryKind {
                None,
                TouchId,
                FaceId,
                OpticId,
            }

            /// What the device can do now.
            #[derive(::uniffi::Record, Clone, Debug, PartialEq, Eq)]
            pub struct BiometricAvailability {
                pub kind: BiometryKind,
                /// A face or finger is enrolled and allowed for this app.
                pub biometrics_usable: bool,
                /// A device passcode is set (the fallback, and the minimum
                /// for an app lock).
                pub passcode_set: bool,
            }

            impl BiometricAvailability {
                /// Whether an app lock can work at all: with biometrics, or
                /// the passcode alone.
                pub fn can_lock(&self) -> bool {
                    self.biometrics_usable || self.passcode_set
                }

                /// The name to show in a setting: "Face ID", "Touch ID",
                /// "Optic ID", or "Passcode".
                pub fn name(&self) -> &'static str {
                    match self.kind {
                        BiometryKind::FaceId if self.biometrics_usable => "Face ID",
                        BiometryKind::TouchId if self.biometrics_usable => "Touch ID",
                        BiometryKind::OpticId if self.biometrics_usable => "Optic ID",
                        _ => "Passcode",
                    }
                }
            }

            /// How an authentication ended.
            #[derive(::uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
            pub enum AuthenticationResult {
                Authenticated,
                /// The person (or the system, or another app) cancelled.
                Cancelled,
                /// Too many failed attempts: biometrics are locked until the
                /// passcode is entered.
                LockedOut,
                /// No biometrics or passcode, or the app's permission was
                /// refused.
                Unavailable,
                Failed,
            }

            /// Implemented by the host (iOS: `LAContext`).
            #[::uniffi::export(with_foreign)]
            #[::async_trait::async_trait]
            pub trait BiometricAuthenticator: Send + Sync {
                fn availability(&self) -> BiometricAvailability;
                /// Asks for Face ID or Touch ID, falling back to the
                /// passcode. `reason` is shown under Touch ID (Face ID shows
                /// the usage text from forge.toml).
                async fn authenticate(&self, reason: String) -> AuthenticationResult;
            }

            #[::uniffi::export]
            pub fn install_native_biometrics(authenticator: Arc<dyn BiometricAuthenticator>) {
                BiometricsService::new(authenticator).install();
            }

            /// Biometrics for Rust code. Cheap to clone.
            #[derive(Clone)]
            pub struct BiometricsService {
                authenticator: Arc<dyn BiometricAuthenticator>,
            }

            $crate::__native_slot!(BiometricsService, "biometrics");

            impl BiometricsService {
                pub fn new(authenticator: Arc<dyn BiometricAuthenticator>) -> Self {
                    Self { authenticator }
                }

                pub fn availability(&self) -> BiometricAvailability {
                    self.authenticator.availability()
                }

                /// Confirms it is the device's owner before something
                /// sensitive (showing a secret, deleting everything).
                /// `reason` finishes the sentence "… needs to": "show your
                /// recovery key".
                pub async fn confirm(
                    &self,
                    reason: &str,
                ) -> Result<AuthenticationResult, super::NativeServiceError> {
                    if reason.trim().is_empty() {
                        return Err(super::NativeServiceError::InvalidInput {
                            reason: "say why the app asks (shown under Touch ID)".into(),
                        });
                    }
                    if !self.availability().can_lock() {
                        return Ok(AuthenticationResult::Unavailable);
                    }
                    Ok(self.authenticator.authenticate(reason.to_string()).await)
                }

                pub fn for_tests(
                    availability: BiometricAvailability,
                ) -> (Self, Arc<testing::FakeBiometrics>) {
                    let fake = Arc::new(testing::FakeBiometrics::new(availability));
                    (Self::new(fake.clone()), fake)
                }
            }

            pub mod testing {
                use super::*;
                use ::std::sync::Mutex;

                /// Answers every request with `result`, and records reasons.
                pub struct FakeBiometrics {
                    availability: BiometricAvailability,
                    pub result: Mutex<AuthenticationResult>,
                    pub reasons: Mutex<Vec<String>>,
                }

                impl FakeBiometrics {
                    pub fn new(availability: BiometricAvailability) -> Self {
                        Self {
                            availability,
                            result: Mutex::new(AuthenticationResult::Authenticated),
                            reasons: Mutex::new(Vec::new()),
                        }
                    }
                }

                #[::async_trait::async_trait]
                impl BiometricAuthenticator for FakeBiometrics {
                    fn availability(&self) -> BiometricAvailability {
                        self.availability.clone()
                    }
                    async fn authenticate(&self, reason: String) -> AuthenticationResult {
                        self.reasons.lock().unwrap().push(reason);
                        *self.result.lock().unwrap()
                    }
                }
            }
        }
    };
}
