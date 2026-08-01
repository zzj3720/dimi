/**
 * Vendor-name gate probe — `src/**` must never branch on the legacy vendor id
 * `'dimi'`. Provider-specific behavior belongs in a `Provider` definition;
 * consumers select providers by their public id and capabilities.
 *
 * Full-line comments (`//`, `/* ...`, JSDoc `* ...`) are not code and may
 * quote the legacy v1 gate as parity documentation; they are skipped.
 * Brand/env names (`DIMI_CODE_*`, `DIMI_MODEL_*`) and `'dimi'` as data
 * (config values, telemetry fields, registration ids) do not match the
 * patterns — verified against the whole `src/` tree.
 */

import { readdirSync, readFileSync, statSync } from "node:fs";
import { dirname, join, relative } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

const __dirname = dirname(fileURLToPath(import.meta.url));
const SRC_ROOT = join(__dirname, "..", "..", "src");

/**
 * Branching on the vendor id: `=== 'dimi'` / `== 'dimi'` / `!== 'dimi'` /
 * `!= 'dimi'` (either operand order) and `case 'dimi':`.
 */
const VENDOR_GATE_RE = /[!=]==?\s*'dimi'|'dimi'\s*[!=]==?|\bcase\s+'dimi'\s*:/;

interface GateHit {
  readonly file: string;
  readonly line: number;
  readonly text: string;
}

function walk(dir: string): string[] {
  const out: string[] = [];
  for (const entry of readdirSync(dir)) {
    const abs = join(dir, entry);
    const st = statSync(abs);
    if (st.isDirectory()) {
      out.push(...walk(abs));
    } else if (abs.endsWith(".ts")) {
      out.push(abs);
    }
  }
  return out;
}

/** Full-line comments may quote the legacy gate as parity documentation. */
function isCommentLine(line: string): boolean {
  const trimmed = line.trimStart();
  return trimmed.startsWith("//") || trimmed.startsWith("/*") || trimmed.startsWith("*");
}

function findVendorGates(source: string, file: string): GateHit[] {
  const hits: GateHit[] = [];
  const lines = source.split("\n");
  for (const [index, line] of lines.entries()) {
    if (isCommentLine(line)) continue;
    VENDOR_GATE_RE.lastIndex = 0;
    if (VENDOR_GATE_RE.test(line)) {
      hits.push({ file, line: index + 1, text: line.trim() });
    }
  }
  return hits;
}

describe("vendor-name gates", () => {
  it("flags vendor compares and switch cases in code", () => {
    const hits = findVendorGates(
      [
        `if (provider.type === 'dimi') return;`,
        `if (provider?.type !== 'dimi' || provider.oauth === undefined) return;`,
        `const managed = 'dimi' === vendor;`,
        `switch (type) { case 'dimi': break; }`,
        `if (type == 'dimi') return;`,
      ].join("\n"),
      "fixture.ts",
    );
    expect(hits.map((hit) => hit.line)).toEqual([1, 2, 3, 4, 5]);
  });

  it("ignores comments, brand/env names, and dimi as data", () => {
    const hits = findVendorGates(
      [
        "// v1 `provider.type === 'dimi'` gate restored.",
        " * `provider.type === 'dimi'` parity): strict validation",
        "/* legacy: provider.type === 'dimi' */",
        "const home = process.env.DIMI_CODE_HOME;",
        `const event = { provider_type: 'dimi' };`,
        `const provider = { type: 'dimi', oauth };`,
        `registerProviderDefinition({ id: 'dimi', ...rest });`,
      ].join("\n"),
      "fixture.ts",
    );
    expect(hits).toEqual([]);
  });

  it("finds no vendor-name gates in src", () => {
    const hits = walk(SRC_ROOT).flatMap((file) =>
      findVendorGates(readFileSync(file, "utf8"), relative(SRC_ROOT, file)),
    );
    expect(
      hits.map((hit) => `${hit.file}:${hit.line} ${hit.text}`),
      "vendor-name gate found — keep provider-specific behavior in the Provider definition",
    ).toEqual([]);
  });
});
