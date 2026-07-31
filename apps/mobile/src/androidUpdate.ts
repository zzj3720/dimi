import * as Application from "expo-application";
import { type NativeModule, requireNativeModule } from "expo";
import { File, Paths } from "expo-file-system";
import * as IntentLauncher from "expo-intent-launcher";
import { Platform } from "react-native";

import {
  updateManifestSchema,
  type UpdatePlatform,
} from "./update";

const INTERNAL_PACKAGE = "org.k3720.mobile.internal";
const MANIFEST_URL = "https://install.k.test.3720.org/android/internal/latest.json";
const APK_MIME = "application/vnd.android.package-archive";
const FLAG_GRANT_READ_URI_PERMISSION = 1;

interface K3720UpdateModule extends NativeModule {
  sha256(uri: string): Promise<string>;
}

export function createAndroidUpdatePlatform(): UpdatePlatform | undefined {
  if (Platform.OS !== "android" || Application.applicationId !== INTERNAL_PACKAGE) return undefined;
  const nativeUpdate = requireNativeModule<K3720UpdateModule>("K3720Update");

  return {
    currentVersionCode() {
      const value = Number(Application.nativeBuildVersion);
      if (!Number.isSafeInteger(value)) throw new Error("Installed Android build number is unavailable.");
      return value;
    },
    async fetchManifest() {
      const response = await fetch(MANIFEST_URL, { cache: "no-store" });
      if (!response.ok) throw new Error(`Update check failed (${response.status}).`);
      return updateManifestSchema.parse(await response.json());
    },
    async downloadAndVerify(manifest) {
      const target = new File(Paths.cache, `k-3720-internal-${manifest.versionCode}.apk`);
      const file = await File.downloadFileAsync(manifest.apk.url, target, { idempotent: true });
      if (file.size !== manifest.apk.size) throw new Error("Downloaded update has the wrong size.");
      const actualHash = await nativeUpdate.sha256(file.uri);
      if (actualHash !== manifest.apk.sha256) throw new Error("Downloaded update failed verification.");
      return file.uri;
    },
    async install(uri) {
      const file = new File(uri);
      await IntentLauncher.startActivityAsync("android.intent.action.VIEW", {
        data: file.contentUri,
        type: APK_MIME,
        flags: FLAG_GRANT_READ_URI_PERMISSION,
      });
    },
  };
}
