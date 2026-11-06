//! The one JNI entry point: `ForgeAndroid.init(context)`.
//!
//! Everything else crosses the boundary through UniFFI (JNA), which cannot
//! pass a JVM object. The HTTPS certificate verifier needs the app `Context`
//! on Android (see `forge_net::platform`), so the generated Kotlin bindings
//! ship a small `org.forgemobile.ffi.ForgeAndroid` object whose
//! `external fun nativeInit` lands here. Hosts call `ForgeAndroid.init` once
//! at startup; repeated calls are harmless.
//!
//! The export is also built for desktop hosts (not iOS), where there is
//! nothing to initialise: that lets the Android JVM unit tests check that the
//! Kotlin declaration and this symbol really match.

use jni::errors::ThrowRuntimeExAndDefault;
use jni::objects::{JClass, JObject};
use jni::EnvUnowned;

/// `ForgeAndroid.nativeInit(context: Context): String?`. Returns `null` on
/// success or the reason `https` stays disabled, so Kotlin can raise its own
/// typed exception rather than a bare `RuntimeException`.
#[unsafe(no_mangle)]
pub extern "system" fn Java_org_forgemobile_ffi_ForgeAndroid_nativeInit<'caller>(
    mut unowned_env: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    context: JObject<'caller>,
) -> JObject<'caller> {
    unowned_env
        .with_env(|env| -> jni::errors::Result<JObject<'caller>> {
            match init(env, context) {
                Ok(()) => Ok(JObject::null()),
                Err(reason) => Ok(env.new_string(reason)?.into()),
            }
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

#[cfg(target_os = "android")]
fn init(env: &mut jni::Env<'_>, context: JObject<'_>) -> Result<(), String> {
    forge_net::platform::android::init(env, context).map_err(|err| err.to_string())
}

/// Off Android the platform verifier needs no setup; only reject what would
/// also fail on a device.
#[cfg(not(target_os = "android"))]
fn init(_env: &mut jni::Env<'_>, context: JObject<'_>) -> Result<(), String> {
    if context.is_null() {
        return Err("ForgeAndroid.init was given a null Context".into());
    }
    Ok(())
}
