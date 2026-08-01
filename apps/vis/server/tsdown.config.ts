import { defineConfig } from "tsdown";

export default defineConfig({
  entry: { server: "src/index.ts" },
  format: ["esm"],
  outDir: "dist",
  clean: true,
  external: ["@dimi-agent/agent-core-v2", "@dimi-agent/kaos"],
});
