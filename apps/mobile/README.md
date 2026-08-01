# k-3720 Android client

The Android app connects to a local runtime through an end-to-end encrypted remote bridge. The relay forwards opaque ciphertext and cannot read prompts or responses.

## Connect a phone

1. Run a relay with a public `wss://` endpoint:

   ```sh
   HOST=0.0.0.0 PORT=8787 vp run @dimi-agent/relay#start
   ```

2. Connect the local runtime to that relay:

   ```sh
   vp run dev:cli -- remote --relay wss://relay.example.com --name "My runtime"
   ```

   An installed compatibility build can use `kimi remote` instead.

3. Open the Android app and scan the QR code printed by the command. The same screen also accepts the printed `k-3720://pair?...` URI.

The pairing identity is stored in Android secure storage. Later bridge restarts reconnect without pairing again.

## Android build

Generate the native project and build a sideloadable APK:

```sh
vp exec expo prebuild --platform android
cd apps/mobile/android
./gradlew assembleRelease
```

The APK is written to:

```text
apps/mobile/android/app/build/outputs/apk/release/app-release.apk
```

The generated project uses the Android debug signing key for local sideloading. Configure a private release key before publishing through an app store.

Install the APK over USB:

```sh
adb install -r apps/mobile/android/app/build/outputs/apk/release/app-release.apk
```

## Internal Android channel

Internal builds use the separate package `org.k3720.mobile.internal`, a stable private signing key, monotonically increasing Android version codes, and the `arm64-v8a` ABI used by current Android phones. The build requires these local-only variables:

```text
K3720_ANDROID_KEYSTORE_FILE
K3720_ANDROID_KEYSTORE_PASSWORD
K3720_ANDROID_KEY_ALIAS
K3720_ANDROID_KEY_PASSWORD
K3720_VERSION_CODE
```

Build and publish from the repository root:

```sh
K3720_CHANNEL=internal K3720_VERSION_CODE=2 vp run @dimi-agent/mobile#build:android:internal
vp run @dimi-agent/distribution#publish:android -- \
  --apk apps/mobile/android/app/build/outputs/apk/release/app-release.apk \
  --version 0.1.0 --version-code 2 --commit "$(git rev-parse HEAD)"
```

The install page is `https://install.k.test.3720.org`. Installed internal builds check `https://install.k.test.3720.org/android/internal/latest.json`, download a compatible newer APK, verify its size and SHA-256, then open the Android package installer for user confirmation. Pairing identity remains in Android secure storage across same-package upgrades.

## Current mobile behavior

- Session list and stable transcript synchronization
- Activity hints without token-level streaming
- Prompt, steer, and cancel
- Approval and question responses
- Automatic relay reconnect and session reload
- Long code blocks open on a separate screen
- Verified internal APK updates with Android installer confirmation
