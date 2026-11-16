//! Android's shared system memory at 3D admission and allocation checkpoints.

use std::sync::OnceLock;

use orqwa_app::memory::MemoryInfo;
use orqwa_devices::android::jni::objects::{JClass, JObject, JValue};
use orqwa_devices::android::jni::refs::{Global, Reference};
use orqwa_devices::android::jni::{self, JavaVM, Outcome, jni_sig, jni_str};

// ActivityManager belongs to the application, so this reference survives activity recreation.
static MANAGER: OnceLock<Global<JObject<'static>>> = OnceLock::new();

/// Called by the APK plugin before Godot opens the extension. A missing service stays an
/// explicit read failure: Android must never accidentally use the desktop's unchecked path.
#[unsafe(no_mangle)]
pub extern "system" fn Java_org_orqwa_android_OrqwaAndroid_initMemory<'caller>(
    mut env: jni::EnvUnowned<'caller>,
    _class: JClass<'caller>,
    manager: JObject<'caller>,
) {
    let outcome = env
        .with_env(|env| -> jni::errors::Result<()> {
            let manager = env.new_global_ref(&manager)?;
            if manager.is_null() {
                return Err(jni::errors::Error::NullPtr("Android ActivityManager"));
            }
            let _ = MANAGER.set(manager);
            Ok(())
        })
        .into_outcome();
    orqwa_devices::android::log(&match outcome {
        Outcome::Ok(()) => "Memory checks ready".to_owned(),
        Outcome::Err(error) => format!("Memory checks not ready: {error}"),
        Outcome::Panic(_) => "Memory checks not ready: initialization panicked".to_owned(),
    });
}

/// ActivityManager.MemoryInfo's availMem accounts for reclaimable memory; the app policy
/// separately retains system headroom. The Java heap's memoryClass excludes native/GPU
/// allocations and is not a budget for Godot.
pub(super) fn free() -> Option<MemoryInfo> {
    let result = (|| -> jni::errors::Result<MemoryInfo> {
        let manager = MANAGER
            .get()
            .ok_or(jni::errors::Error::NullPtr("Android ActivityManager"))?;
        JavaVM::singleton()?.attach_current_thread(|env| {
            let info = env.new_object(
                jni_str!(
                    jni = orqwa_devices::android::jni,
                    "android/app/ActivityManager$MemoryInfo"
                ),
                jni_sig!(jni = orqwa_devices::android::jni, "()V"),
                &[],
            )?;
            env.call_method(
                manager,
                jni_str!(jni = orqwa_devices::android::jni, "getMemoryInfo"),
                jni_sig!(
                    jni = orqwa_devices::android::jni,
                    "(Landroid/app/ActivityManager$MemoryInfo;)V"
                ),
                &[JValue::Object(&info)],
            )?;
            let available = env
                .get_field(
                    &info,
                    jni_str!(jni = jni, "availMem"),
                    jni_sig!(jni = jni, "J"),
                )?
                .j()?;
            let total = env
                .get_field(
                    &info,
                    jni_str!(jni = jni, "totalMem"),
                    jni_sig!(jni = jni, "J"),
                )?
                .j()?;
            let threshold = env
                .get_field(
                    &info,
                    jni_str!(jni = jni, "threshold"),
                    jni_sig!(jni = jni, "J"),
                )?
                .j()?;
            let low = env
                .get_field(
                    &info,
                    jni_str!(jni = jni, "lowMemory"),
                    jni_sig!(jni = jni, "Z"),
                )?
                .z()?;
            // A negative Java value is invalid, never an enormous unsigned memory budget.
            Ok(MemoryInfo {
                available: u64::try_from(available).unwrap_or(0),
                total: u64::try_from(total).unwrap_or(0),
                threshold: u64::try_from(threshold).unwrap_or(u64::MAX),
                low,
            })
        })
    })();
    result
        .inspect_err(|error| tracing::warn!(%error, "cannot read Android memory"))
        .ok()
}
