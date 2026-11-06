//! One-time platform setup the HTTPS transport needs.
//!
//! [`crate::UreqClient`] checks certificates with `rustls-platform-verifier`,
//! which asks the operating system whether a server's certificate is
//! trusted. On iOS, macOS and Linux that works with no setup. On Android the
//! verifier calls the platform's `X509TrustManager` through JNI, so it needs
//! the app's `Context` once, before the first `https` request. Forge hosts do
//! that with `ForgeAndroid.init(context)` (Kotlin, shipped with the generated
//! bindings), which calls [`android::init`] through `forge-ffi`.
//!
//! Until that has happened an `https` request fails at once with
//! [`NetError::TlsNotInitialised`]. It never falls back to an unverified
//! connection, and it never reaches the verifier, which would panic.

use std::sync::atomic::{AtomicBool, Ordering};

use crate::types::NetError;

static ANDROID_READY: AtomicBool = AtomicBool::new(false);

/// Whether `https` requests can be verified on this platform yet. Always
/// `true` off Android.
pub fn https_ready() -> bool {
    !cfg!(target_os = "android") || ANDROID_READY.load(Ordering::Acquire)
}

/// Refuses an `https` URL while the verifier is not ready. Plain `http`
/// never touches the verifier, so it is always allowed.
pub(crate) fn ensure_ready_for(url: &str) -> Result<(), NetError> {
    check_ready(url, https_ready())
}

fn check_ready(url: &str, ready: bool) -> Result<(), NetError> {
    let https = url
        .get(..8)
        .is_some_and(|scheme| scheme.eq_ignore_ascii_case("https://"));
    if https && !ready {
        return Err(NetError::TlsNotInitialised {
            reason: "the Android certificate verifier has not been initialised; call \
                     ForgeAndroid.init(context) before the first https request"
                .into(),
        });
    }
    Ok(())
}

/// Android: hands the app `Context` to the certificate verifier.
#[cfg(target_os = "android")]
pub mod android {
    use jni::objects::{JClassLoader, JObject};
    use jni::{jni_sig, jni_str, Env};

    use super::{Ordering, ANDROID_READY};
    use crate::types::NetError;

    /// The Kotlin half of the verifier (`org.rustls:rustls-platform-verifier`,
    /// an Android library the app must depend on).
    const VERIFIER_CLASS: &str = "org.rustls.platformverifier.CertificateVerifier";

    /// Initialises the verifier with the app `Context`. Safe to call more
    /// than once and from several threads: the first successful call wins
    /// and later calls return `Ok` without doing anything.
    ///
    /// Fails, leaving `https` disabled, when `context` is null or the
    /// verifier's Kotlin class is missing from the app (the Gradle
    /// dependency was left out or stripped by R8).
    pub fn init(env: &mut Env<'_>, context: JObject<'_>) -> Result<(), NetError> {
        if super::https_ready() {
            return Ok(());
        }
        if context.is_null() {
            return Err(not_initialised(
                "ForgeAndroid.init was given a null Context",
            ));
        }
        check_verifier_class(env, &context)?;
        rustls_platform_verifier::android::init_with_env(env, context)
            .map_err(|err| not_initialised(format!("verifier init failed: {err}")))?;
        ANDROID_READY.store(true, Ordering::Release);
        Ok(())
    }

    /// Loads the verifier's Kotlin class through the app class loader, the
    /// way the verifier itself will. Failing here, with a clear message,
    /// beats every TLS handshake failing later.
    fn check_verifier_class(env: &mut Env<'_>, context: &JObject<'_>) -> Result<(), NetError> {
        let result = (|| -> jni::errors::Result<()> {
            let loader = env
                .call_method(
                    context,
                    jni_str!("getClassLoader"),
                    jni_sig!(() -> JClassLoader),
                    &[],
                )?
                .l()?;
            let loader = env.cast_local::<JClassLoader>(loader)?;
            let name = env.new_string(VERIFIER_CLASS)?;
            loader.load_class(env, name)?;
            Ok(())
        })();
        result.map_err(|err| {
            if env.exception_check() {
                env.exception_clear();
            }
            not_initialised(format!(
                "{VERIFIER_CLASS} is not in the app ({err}); add the \
                 org.rustls:rustls-platform-verifier dependency (see docs/android-setup.md)"
            ))
        })
    }

    fn not_initialised(reason: impl Into<String>) -> NetError {
        NetError::TlsNotInitialised {
            reason: reason.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn https_waits_for_init_and_http_never_does() {
        for url in ["https://chat.example/", "HTTPS://chat.example/"] {
            assert!(matches!(
                check_ready(url, false),
                Err(NetError::TlsNotInitialised { .. })
            ));
            assert_eq!(check_ready(url, true), Ok(()));
        }
        assert_eq!(check_ready("http://10.0.2.2:8787/", false), Ok(()));
    }

    #[test]
    #[cfg(not(target_os = "android"))]
    fn hosts_other_than_android_need_no_init() {
        assert_eq!(https_ready(), !cfg!(target_os = "android"));
        assert_eq!(ensure_ready_for("https://example.com/"), Ok(()));
    }
}
