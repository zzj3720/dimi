import { readFileSync } from "node:fs";
import { join, resolve } from "node:path";

import { defineConfig } from "tsdown";

import { rawTextPlugin } from "../../build/raw-text-plugin.mjs";
const appRoot = import.meta.dirname;

function jsoncParserEsmEntry(): string {
  // See tsdown.native.config.ts: jsonc-parser's CJS entry requires ./impl/*
  // via a UMD factory that the published bundle cannot satisfy. Pin the ESM
  // entry so static imports are inlined.
  const coreRoot = resolve(appRoot, "../../packages/agent-core-v2");
  const requireFromCore = createRequire(join(coreRoot, "package.json"));
  const pkgPath = requireFromCore.resolve("jsonc-parser/package.json");
  const pkg = JSON.parse(readFileSync(pkgPath, "utf-8")) as { module?: string };
  if (typeof pkg.module !== "string") {
    throw new Error("jsonc-parser has no ESM module entry");
  }
  return resolve(join(pkgPath, ".."), pkg.module);
}

export default defineConfig({
  entry: ["./src/main.ts"],
  format: ["esm"],
  outDir: "dist",
  clean: true,
  dts: false,
  hash: false,
  banner: {
    js: [
      "#!/usr/bin/env node",
      "import { fileURLToPath as __cjsShimFileURLToPath } from 'node:url';",
      "import { dirname as __cjsShimDirname } from 'node:path';",
      "const __filename = __cjsShimFileURLToPath(import.meta.url);",
      "const __dirname = __cjsShimDirname(__filename);",
    ].join("\n"),
  },
  plugins: [rawTextPlugin()],
  alias: {
    "@": resolve(appRoot, "src"),
    "jsonc-parser": jsoncParserEsmEntry(),
  },
  deps: {
    onlyBundle: false,
  },
  outputOptions: {
    codeSplitting: false,
    entryFileNames: "main.mjs",
  },
});
