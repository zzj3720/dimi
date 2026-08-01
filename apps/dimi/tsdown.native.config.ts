import { readFileSync } from "node:fs";
import { builtinModules, createRequire } from "node:module";
import { join, resolve } from "node:path";

import { defineConfig } from "tsdown";

import { rawTextPlugin } from "../../build/raw-text-plugin.mjs";
const appRoot = import.meta.dirname;
const requireFromCli = createRequire(import.meta.url);
const packageJson = JSON.parse(
  readFileSync(new URL("./package.json", import.meta.url), "utf-8"),
) as { version: string };

const builtins = new Set([...builtinModules, ...builtinModules.map((name) => `node:${name}`)]);
const optionalNativeDependencies = new Set(["cpu-features"]);

function jsoncParserEsmEntry(): string {
  // jsonc-parser's CJS entry (`main`) requires `./impl/*` at runtime via a
  // UMD factory — a relative require the SEA bundle cannot satisfy. Its ESM
  // entry uses static imports, which esbuild inlines correctly, so pin the
  // ESM build here. Resolve from agent-core-v2 (the direct dependant) so we
  // don't hardcode a pnpm store path.
  const coreRoot = resolve(appRoot, "../../packages/agent-core-v2");
  const requireFromCore = createRequire(join(coreRoot, "package.json"));
  const pkgPath = requireFromCore.resolve("jsonc-parser/package.json");
  const pkg = JSON.parse(readFileSync(pkgPath, "utf-8")) as { module?: string };
  if (typeof pkg.module !== "string") {
    throw new Error("jsonc-parser has no ESM module entry");
  }
  // `pkg.module` is relative to the package root (package.json's directory).
  return resolve(join(pkgPath, ".."), pkg.module);
}

function shouldAlwaysBundle(id: string): boolean {
  if (builtins.has(id) || id.startsWith("node:")) return false;
  if (optionalNativeDependencies.has(id)) return false;
  // Everything else is force-bundled, which covers `@dimi-agent/*` (incl.
  // vis-server for `dimi vis`) plus its transitive `hono` / `@hono/node-server`
  // — so the SEA bundle is self-contained (check-bundle.mjs enforces this).
  return true;
}

function buildTarget(): string {
  return process.env["DIMI_CODE_BUILD_TARGET"] ?? `${process.platform}-${process.arch}`;
}

export default defineConfig({
  entry: ["./src/main.ts"],
  format: ["cjs"],
  outDir: "dist-native/intermediates",
  clean: true,
  dts: false,
  fixedExtension: true,
  hash: false,
  platform: "node",
  target: "node24",
  banner: { js: "#!/usr/bin/env node" },
  plugins: [rawTextPlugin()],
  alias: {
    "@": resolve(appRoot, "src"),
    // jsonc-parser's CJS entry (`main`) requires `./impl/*` at runtime via a
    // UMD factory — a relative require the SEA bundle cannot satisfy. Its ESM
    // entry uses static imports, which esbuild inlines correctly, so pin the
    // ESM build here (see jsoncParserEsmEntry).
    "jsonc-parser": jsoncParserEsmEntry(),
  },
  define: {
    __KIMI_CODE_VERSION__: JSON.stringify(packageJson.version),
    __KIMI_CODE_CHANNEL__: JSON.stringify(process.env["DIMI_CODE_CHANNEL"] ?? ""),
    __KIMI_CODE_COMMIT__: JSON.stringify(process.env["DIMI_CODE_COMMIT"] ?? ""),
    __KIMI_CODE_BUILD_TARGET__: JSON.stringify(buildTarget()),
    __KIMI_CODE_NATIVE_BUNDLE__: "true",
  },
  deps: {
    alwaysBundle: shouldAlwaysBundle,
    neverBundle: [...optionalNativeDependencies],
    onlyBundle: false,
  },
  outputOptions: {
    codeSplitting: false,
    entryFileNames: "main.cjs",
  },
  checks: {
    legacyCjs: false,
  },
});
