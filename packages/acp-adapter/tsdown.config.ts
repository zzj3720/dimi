import { defineConfig } from "tsdown";

export default defineConfig({
  entry: ["./src/index.ts"],
  format: ["esm"],
  dts: true,
  outDir: "dist",
  clean: true,
  deps: {
    neverBundle: [
      "@agentclientprotocol/sdk",
      "@dimi-agent/agent-core-v2",
      "@dimi-agent/dimi-sdk",
      "@dimi-agent/kaos",
    ],
  },
});
