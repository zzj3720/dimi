import { fileURLToPath } from "node:url";

import { defineConfig } from "tsdown";

import { rawTextPlugin } from "../../build/raw-text-plugin.mjs";

export default defineConfig({
  entry: ["./src/index.ts"],
  format: ["esm"],
  dts: false,
  outDir: "dist",
  clean: true,
  plugins: [rawTextPlugin()],
  banner: {
    js: [
      "import { fileURLToPath as __cjsShimFileURLToPath } from 'node:url';",
      "import { dirname as __cjsShimDirname } from 'node:path';",
      "const __filename = __cjsShimFileURLToPath(import.meta.url);",
      "const __dirname = __cjsShimDirname(__filename);",
    ].join("\n"),
  },
  alias: {
    "@dimi-agent/agent-core-v2": fileURLToPath(
      new URL("../agent-core-v2/src/index.ts", import.meta.url),
    ),
    "@dimi-agent/kaos": fileURLToPath(new URL("../kaos/src/index.ts", import.meta.url)),
    "@dimi-agent/dimi-oauth": fileURLToPath(
      new URL("../oauth/src/index.ts", import.meta.url),
    ),
  },
  deps: {
    alwaysBundle: [/^@dimi-agent\//],
    neverBundle: [],
  },
});
