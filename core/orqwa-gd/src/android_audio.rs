//! AAudio's Android context when CPAL is embedded in Godot rather than a Rust Android runtime.

use jni::objects::{JClass, JObject};
use jni::refs::{Global, Reference};
use orqwa_devices::android::jni::{self, Outcome};
use std::sync::OnceLock;

// The application context survives activity recreation. Its global reference and VM remain
// valid for the process, including while the bounded native voice worker releases streams.
static CONTEXT: OnceLock<Global<JObject<'static>>> = OnceLock::new();

/// Called by the APK plugin before the extension can enumerate or open native voice devices.
#[unsafe(no_mangle)]
pub extern "system" fn Java_org_orqwa_android_OrqwaAndroid_initVoice<'caller>(
    mut env: jni::EnvUnowned<'caller>,
    _class: JClass<'caller>,
    context: JObject<'caller>,
) {
    let outcome = env
        .with_env(|env| -> jni::errors::Result<()> {
            let global = env.new_global_ref(&context)?;
            if global.is_null() {
                return Err(jni::errors::Error::NullPtr(
                    "Android voice application context",
                ));
            }
            let vm = env.get_java_vm()?;
            CONTEXT.get_or_init(|| {
                // SAFETY: the VM is the current Android VM, and the retained global reference
                // owns the application context. OnceLock runs this before publishing it once.
                unsafe {
                    ndk_context::initialize_android_context(
                        vm.get_raw().cast(),
                        global.as_raw().cast(),
                    )
                };
                global
            });
            Ok(())
        })
        .into_outcome();
    orqwa_devices::android::log(&match outcome {
        Outcome::Ok(()) => "Voice audio context ready".to_owned(),
        Outcome::Err(error) => format!("Voice audio context not ready: {error}"),
        Outcome::Panic(_) => "Voice audio context not ready: initialization panicked".to_owned(),
    });
}
