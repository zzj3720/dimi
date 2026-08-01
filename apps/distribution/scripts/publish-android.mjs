import { createHash } from "node:crypto";
import { createReadStream, unlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { basename, join } from "node:path";
import { spawnSync } from "node:child_process";

const args = parseArgs(process.argv.slice(2));
const apk = required(args, "apk");
const version = required(args, "version");
const versionCode = Number(required(args, "version-code"));
const commit = required(args, "commit");
if (!Number.isSafeInteger(versionCode) || versionCode < 1) throw new Error("Invalid --version-code.");

const hash = createHash("sha256");
let size = 0;
for await (const chunk of createReadStream(apk)) {
  hash.update(chunk);
  size += chunk.length;
}

const apkKey = `android/builds/${versionCode}/k-3720-internal.apk`;
const manifest = {
  schema: 1,
  channel: "internal",
  platform: "android",
  version,
  versionCode,
  minVersionCode: 1,
  publishedAt: new Date().toISOString(),
  commit,
  apk: {
    url: `https://install.k.test.3720.org/${apkKey}`,
    sha256: hash.digest("hex"),
    size,
  },
};

const manifestPath = join(tmpdir(), `k3720-${process.pid}-latest.json`);
writeFileSync(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`, { mode: 0o600 });
try {
  upload(apkKey, apk, "application/vnd.android.package-archive");
  upload("android/internal/latest.json", manifestPath, "application/json");
} finally {
  unlinkSync(manifestPath);
}
process.stdout.write(`${basename(apk)} published as build ${versionCode} (${manifest.apk.sha256}).\n`);

function upload(key, file, contentType) {
  const result = spawnSync(
    "wrangler",
    [
      "r2",
      "object",
      "put",
      `k3720-internal-builds-test/${key}`,
      "--file",
      file,
      "--content-type",
      contentType,
      "--remote",
    ],
    { stdio: "inherit" },
  );
  if (result.status !== 0) throw new Error(`Failed to upload ${key}.`);
}

function parseArgs(values) {
  const parsed = new Map();
  const pairs = values[0] === "--" ? values.slice(1) : values;
  for (let index = 0; index < pairs.length; index += 2) {
    const key = pairs[index]?.replace(/^--/, "");
    const value = pairs[index + 1];
    if (key === undefined || value === undefined) throw new Error("Arguments must be --key value pairs.");
    parsed.set(key, value);
  }
  return parsed;
}

function required(values, key) {
  const value = values.get(key);
  if (value === undefined) throw new Error(`Missing --${key}.`);
  return value;
}
