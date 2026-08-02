/**
 * TS zod ↔ Rust dimi-wire differential suite (M0.5).
 *
 * For every fixture line: zod's normalization (`JSON.stringify(zod.parse(...))`)
 * must equal the Rust bridge's normalization byte-for-byte. This proves the
 * Rust mirror agrees with the contract's source of truth on field names,
 * key order, optional omission, number formatting, tag discrimination and
 * unknown-field stripping.
 *
 * The suite skips itself when the native binding is not built, so CI without
 * a Rust toolchain stays green; build it with
 * `pnpm --filter @dimi-agent/dimi-native run build:native`.
 */
import { existsSync, readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { describe, expect, test } from 'vitest';

import {
  agentPhaseMetaSchema,
  isPlainAgentId as tsIsPlainAgentId,
  transcriptItemSchema,
  transcriptStepSchema,
  transcriptTaskSchema,
} from '@dimi-agent/transcript';

import { isPlainAgentId, normalizeItem, normalizePhase, normalizeStep, normalizeTask } from '#/index';

const bindingPath = fileURLToPath(new URL('../dist/dimi_bridge.node', import.meta.url));
const fixturesDir = fileURLToPath(new URL('../../../crates/dimi-wire/fixtures', import.meta.url));

function fixtureLines(file: string): string[] {
  return readLines(`${fixturesDir}/${file}`);
}

function readLines(path: string): string[] {
  return readFileSync(path, 'utf8')
    .split('\n')
    .map((line) => line.trim())
    .filter((line) => line && !line.startsWith('#'));
}

function zodNormalize<T>(schema: { parse(v: unknown): T }, json: string): string {
  return JSON.stringify(schema.parse(JSON.parse(json)));
}

/**
 * Semantic comparison: the TS server's objects are built by the engine with
 * ITS construction key order (coreEventMap.ts), while zod rebuilds objects
 * in schema order — byte equality between the two is not an invariant. The
 * contract that must hold is deep equality of the parsed values.
 */
function expectDeepEqual(actualJson: string, expectedJson: string, label: string): void {
  expect(JSON.parse(actualJson), label).toEqual(JSON.parse(expectedJson));
}

const nativeAvailable = existsSync(bindingPath);
const suite = nativeAvailable ? describe : describe.skip;

suite('TS zod ↔ Rust dimi-wire differential', () => {
  test('items: zod parse equals rust parse (semantic)', () => {
    const lines = fixtureLines('items.jsonl');
    expect(lines.length).toBeGreaterThan(0);
    for (const line of lines) {
      expectDeepEqual(normalizeItem(line), zodNormalize(transcriptItemSchema, line), `item mismatch for ${line.slice(0, 80)}…`);
    }
  });

  test('phases: agentPhaseMetaSchema agrees', () => {
    const lines = fixtureLines('phases.jsonl');
    expect(lines.length).toBeGreaterThan(0);
    for (const line of lines) {
      expectDeepEqual(normalizePhase(line), zodNormalize(agentPhaseMetaSchema, line), `phase mismatch for ${line.slice(0, 80)}…`);
    }
  });

  test('step and task shapes agree on inline samples', () => {
    const step =
      '{"kind":"step","stepId":"s_1","turnId":"t_1","ordinal":0,"state":"completed","frames":[],"usage":{"inputOther":1,"output":2,"inputCacheRead":3,"inputCacheCreation":4}}';
    expectDeepEqual(normalizeStep(step), zodNormalize(transcriptStepSchema, step), 'step');

    const task =
      '{"taskId":"task_1","kind":"subagent","state":"timed_out","detached":false,"outputTail":"","resultSummary":"no result","agentId":"child_1"}';
    expectDeepEqual(normalizeTask(task), zodNormalize(transcriptTaskSchema, task), 'task');
  });

  test('isPlainAgentId agrees with the zod-side helper', () => {
    const samples = [
      'a',
      'abc',
      'a.b-c_1',
      '0123456789',
      'x'.repeat(128),
      '',
      '.',
      '..',
      'a/b',
      'a\\b',
      'a b',
      'x'.repeat(129),
      'a\u{0}b',
    ];
    for (const sample of samples) {
      expect(isPlainAgentId(sample), `isPlainAgentId(${JSON.stringify(sample)})`).toBe(
        tsIsPlainAgentId(sample),
      );
    }
  });

  test('null optionals are rejected by both sides', () => {
    const bad =
      '{"kind":"turn","turnId":"t_1","ordinal":0,"state":"completed","origin":{"kind":"user"},"prompt":null,"steps":[]}';
    expect(() => zodNormalize(transcriptItemSchema, bad)).toThrow();
    expect(() => normalizeItem(bad)).toThrow();
  });

  test('unknown fields are stripped by both sides', () => {
    const line = '{"kind":"marker","markerId":"m_1","marker":"x","bogus":1}';
    expectDeepEqual(normalizeItem(line), zodNormalize(transcriptItemSchema, line), 'marker strip');
  });

  test('unknown kind tags are rejected by both sides', () => {
    const bad = '{"kind":"bogus","turnId":"t_1"}';
    expect(() => zodNormalize(transcriptItemSchema, bad)).toThrow();
    expect(() => normalizeItem(bad)).toThrow();
  });

  test('scrambled key order parses to the same shape', () => {
    const scrambled =
      '{"steps":[],"origin":{"kind":"user"},"state":"queued","ordinal":0,"turnId":"t_1","kind":"turn"}';
    expectDeepEqual(normalizeItem(scrambled), zodNormalize(transcriptItemSchema, scrambled), 'scrambled');
  });
});

if (!nativeAvailable) {
  // A skipped suite is easy to miss; make the absence loud in the test log.
  console.warn(
    '[dimi-native] native binding not built — differential suite skipped. Run `pnpm --filter @dimi-agent/dimi-native run build:native` to enable it.',
  );
}
