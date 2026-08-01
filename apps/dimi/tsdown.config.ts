import { resolve } from "node:path";

import { defineConfig } from "tsdown";

import { rawTextPlugin } from "../../build/raw-text-plugin.mjs";
const appRoot = import.meta.dirname;

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
  },
  deps: {
    onlyBundle: false,
  },
  outputOptions: {
    codeSplitting: false,
    entryFileNames: "main.mjs",
  },
});
