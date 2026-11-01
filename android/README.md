# Android (ADR 0024)

ORQWA as an APK for Android tablets, built in a container of its own so the dev image and the
checks stay as they are.

```sh
scripts/android.sh scripts/build-android.sh      # → dist/ORQWA-android-arm64.apk (debug-signed)
```

| Part | What it is |
|---|---|
| `Dockerfile` | The Android image: the dev image plus OpenJDK 21, the SDK (platform 36, build tools 36.1.0), the NDK, FFmpeg for Android and Godot's Android templates. The debug key is made once and kept in the `orqwa-gradle` volume, so updates install over the APK on a device |
| `plugin/` | ORQWA's Godot Android plugin: loads ORQWA's library early so btleplug finds its Java classes (compiled in from the btleplug crate), initializes voice's Android context, asks for Bluetooth |
| `godot/` | What only the APK build adds to its copy of the project: the export preset and the editor addon that puts the plugin into the export |
| `stage.py` | Adds those, the extension's Android library and Android's texture formats to the copy |

The project in `app/` is never changed: `scripts/build-android.sh` copies it to a build folder in
the container first.

The Rust extension, Android plugin and APK export target the documented Android 10 minimum
(API 29). Group voice links Android's AAudio library, which is absent from the API 24 native
target. The image's FFmpeg libraries can still be built against API 24 and linked into this
newer target; no SDK or NDK update is needed for this alignment.

The plugin initializes CPAL's existing `ndk-context` before the extension can enumerate or
open audio. Rust retains one global reference to the application context, surviving activity
recreation without keeping an old Activity alive. Initialization opens no audio stream and
requests no microphone permission; capture still requires explicit consent.

Received group activities are staged and validated on their destination filesystem. Android's
emulated shared storage rejects hard links, so publication falls back to the kernel's atomic
`renameat2(RENAME_NOREPLACE)` operation. The syscall works at API 29, where Bionic does not yet
export its API-30 `renameat2` wrapper. It never overwrites an existing activity; conflicts are
verified or rejected. No broad storage permission or private-library relocation is needed.

## Installing on a tablet

With wireless debugging paired (`adb pair`, then `adb connect <address>`):

```sh
adb install -r dist/ORQWA-android-arm64.apk
adb logcat -s ORQWA godot
```

## Release signing

Tagged releases are signed in CI with the release key from the secrets `ANDROID_KEYSTORE` and
`ANDROID_KEYSTORE_PASSWORD` (alias `torqa`). Android installs an update only over an APK with the
same key, so losing it would make every rider uninstall, and lose their rides, to update. Its
owner keeps a copy and the password outside GitHub. Locally, `ORQWA_RELEASE_KEYSTORE` and
`ORQWA_RELEASE_KEYSTORE_PASSWORD` sign the same way.

## Removing Android support

Everything Android lives here and in a few marked places; to take it out again:

1. Delete `android/`, `scripts/android.sh` and `scripts/build-android.sh`.
2. In `scripts/build-ffmpeg.sh`, remove the `android` branch.
3. In `core/`: delete `orqwa-devices/src/android.rs` and its `mod` line, the
   `cfg(target_os = "android")` `jni` dependency in `orqwa-devices/Cargo.toml`, `jni` in the
   workspace `Cargo.toml`, and `JNI_OnLoad` at the end of `orqwa-gd/src/lib.rs`. In
   `orqwa-update`, drop the Android installer name and `target_os = "android"` from
   `can_install`; `downloaded_update` in `orqwa-app` and `orqwa-gd` may stay.
4. Remove the Docker volume `orqwa-gradle` and the image `orqwa-android`; in CI, the `android`
   job, `android` in the release's `needs`, the APK in `scripts/release/publish.sh` and its
   test, and the two `ANDROID_KEYSTORE` secrets.
5. In `app/ui/update_dialog.gd`, remove the `ANDROID` plugin branches.
6. Mark ADR 0024 superseded and update `docs/PLAN.md`.
