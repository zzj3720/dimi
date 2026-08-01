import type { ConfigContext, ExpoConfig } from "expo/config";

export default ({ config }: ConfigContext): ExpoConfig => {
  if (config.slug === undefined) throw new Error("Expo slug is missing from app.json.");
  const internal = process.env["K3720_CHANNEL"] === "internal";
  const versionCode = Number(
    process.env["K3720_VERSION_CODE"] ?? config.android?.versionCode ?? 1,
  );
  if (!Number.isSafeInteger(versionCode) || versionCode < 1) {
    throw new Error("K3720_VERSION_CODE must be a positive integer.");
  }

  return {
    ...config,
    name: internal ? "k-3720 Internal" : (config.name ?? "k-3720"),
    slug: config.slug,
    android: {
      ...config.android,
      package: internal ? "org.k3720.mobile.internal" : config.android?.package,
      versionCode,
      permissions: internal ? ["android.permission.REQUEST_INSTALL_PACKAGES"] : [],
    },
    plugins: [
      ...(config.plugins ?? []),
      ...(internal ? ["./plugins/withInternalAndroidSigning.cjs" as const] : []),
    ],
  };
};
