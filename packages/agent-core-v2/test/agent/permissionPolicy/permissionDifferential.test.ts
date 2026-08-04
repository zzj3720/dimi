/**
 * Permission chain TS↔Rust differential gate (A1 architecture review).
 *
 * The Rust engine re-implements the TS `permissionPolicyService` 12-node
 * chain by hand — historically, parity was maintained by human review, and
 * every review round found a new divergence (chain order, missing nodes,
 * scope filtering, yolo handling, empty arg patterns, message text). This
 * suite feeds the SAME `PolicyInput` corpus to both implementations and
 * asserts decision + reason parity, so any TS permission change that is not
 * mirrored in `dimi_engine::permission` turns this suite red.
 *
 * Corpus cases deliberately include every edge a review round has already
 * caught, so regressions on those exact semantics are pinned.
 */
import { mkdir, mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { evaluatePolicy } from "@dimi-agent/dimi-native";

import type { ToolCall } from "#/llmProtocol/message";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { DisposableStore } from "#/_base/di/lifecycle";
import { createServices, type TestInstantiationService } from "#/_base/di/test";
import {
  literalRulePattern,
  matchesGlobRuleSubject,
  matchesPathRuleSubject,
} from "#/tool/rule-match";
import type { ResolvedToolExecutionHookContext } from "#/agent/toolExecutor/toolHooks";
import {
  IHostEnvironment,
  type IHostEnvironment as HostEnvironmentService,
} from "#/os/interface/hostEnvironment";
import { IAgentPermissionModeService } from "#/agent/permissionMode/permissionMode";
import { IAgentPermissionPolicyService } from "#/agent/permissionPolicy/permissionPolicy";
import type { PermissionMode } from "#/agent/permissionPolicy/types";
import { AgentPermissionPolicyService } from "#/agent/permissionPolicy/permissionPolicyService";
import {
  IAgentPermissionRulesService,
  type IAgentPermissionRulesService as PermissionRulesServiceContract,
  type PermissionRule,
} from "#/agent/permissionRules/permissionRules";
import { IAgentScopeContext, makeAgentScopeContext } from "#/agent/scopeContext/scopeContext";
import { ITelemetryService } from "#/app/telemetry/telemetry";
import { ToolAccesses, type ToolAccesses as ToolAccessList } from "#/tool/toolContract";
import { ISessionWorkspaceContext } from "#/session/workspaceContext/workspaceContext";

import { stubPermissionModeService } from "../permissionMode/stubs";
import { recordingTelemetry } from "../../app/telemetry/stubs";

const signal = new AbortController().signal;

interface DifferentialCase {
  readonly name: string;
  readonly mode: PermissionMode;
  readonly toolName: string;
  readonly args: Record<string, unknown>;
  readonly rules?: readonly PermissionRule[];
  readonly sessionApprovalRulePatterns?: readonly string[];
  readonly workspaceDir?: string;
}

const CASES: readonly DifferentialCase[] = [
  // Chain order: whitelist vs fallback in manual mode.
  { name: "manual Read whitelist approves", mode: "manual", toolName: "Read", args: { path: "/workspace/a.txt" } },
  { name: "manual Bash falls back to ask", mode: "manual", toolName: "Bash", args: { command: "echo hi" } },
  { name: "manual Grep whitelist approves", mode: "manual", toolName: "Grep", args: { pattern: "foo", path: "/workspace" } },
  // User deny beats whitelist, auto mode, and allow rules.
  {
    name: "user deny beats whitelist in manual",
    mode: "manual",
    toolName: "Read",
    args: { path: "/workspace/a.txt" },
    rules: [{ decision: "deny", scope: "user", pattern: "Read", reason: "no reads" }],
  },
  {
    name: "user deny beats whitelist in auto",
    mode: "auto",
    toolName: "Read",
    args: { path: "/workspace/a.txt" },
    rules: [{ decision: "deny", scope: "user", pattern: "Read", reason: "no reads" }],
  },
  {
    name: "deny rule beats allow rule regardless of order",
    mode: "manual",
    toolName: "Bash",
    args: { command: "rm -rf /" },
    rules: [
      { decision: "allow", scope: "user", pattern: "Bash" },
      { decision: "deny", scope: "user", pattern: "Bash(rm*)", reason: "no rm" },
    ],
  },
  // Deny message text (fed to the LLM — byte parity matters).
  {
    name: "deny message with reason matches TS",
    mode: "manual",
    toolName: "Bash",
    args: { command: "x" },
    rules: [{ decision: "deny", scope: "user", pattern: "Bash", reason: "blocked" }],
  },
  {
    name: "deny message without reason matches TS",
    mode: "manual",
    toolName: "Bash",
    args: { command: "x" },
    rules: [{ decision: "deny", scope: "user", pattern: "Bash" }],
  },
  // session-runtime rules are NOT user-configured rules (scope filter).
  {
    name: "session-runtime deny rule is not user-configured",
    mode: "manual",
    toolName: "Bash",
    args: { command: "x" },
    rules: [{ decision: "deny", scope: "session-runtime", pattern: "Bash", reason: "runtime" }],
  },
  // Ask and allow rules (ask before allow).
  {
    name: "ask rule asks",
    mode: "manual",
    toolName: "Bash",
    args: { command: "x" },
    rules: [{ decision: "ask", scope: "user", pattern: "Bash" }],
  },
  {
    name: "allow rule approves",
    mode: "manual",
    toolName: "Bash",
    args: { command: "x" },
    rules: [{ decision: "allow", scope: "user", pattern: "Bash" }],
  },
  {
    name: "ask rule beats allow rule",
    mode: "manual",
    toolName: "Bash",
    args: { command: "x" },
    rules: [
      { decision: "allow", scope: "user", pattern: "Bash" },
      { decision: "ask", scope: "user", pattern: "Bash" },
    ],
  },
  // Empty arg pattern degrades to tool-name-only matching.
  {
    name: "empty arg pattern matches by tool name",
    mode: "manual",
    toolName: "Bash",
    args: { command: "anything" },
    rules: [{ decision: "allow", scope: "user", pattern: "Bash()" }],
  },
  // Session history precedes ask/allow and the whitelist.
  {
    name: "session history approves before ask rule",
    mode: "manual",
    toolName: "Bash",
    args: { command: "x" },
    rules: [{ decision: "ask", scope: "user", pattern: "Bash" }],
    sessionApprovalRulePatterns: ["Bash"],
  },
  // Mode posture: auto approves, yolo approves, auto denies AskUserQuestion,
  // yolo does NOT deny AskUserQuestion (adversarial-review P1).
  { name: "auto approves Bash", mode: "auto", toolName: "Bash", args: { command: "x" } },
  { name: "yolo approves Bash", mode: "yolo", toolName: "Bash", args: { command: "x" } },
  { name: "auto denies AskUserQuestion", mode: "auto", toolName: "AskUserQuestion", args: { questions: [] } },
  { name: "yolo allows AskUserQuestion", mode: "yolo", toolName: "AskUserQuestion", args: { questions: [] } },
  // Sensitive file access asks even for whitelisted tools, before approval.
  { name: "manual Read .env asks (sensitive)", mode: "manual", toolName: "Read", args: { path: "/workspace/.env" } },
  { name: "manual Read .env.example approves (exempt)", mode: "manual", toolName: "Read", args: { path: "/workspace/.env.example" } },
  { name: "manual Read id_ed25519 asks (sensitive)", mode: "manual", toolName: "Read", args: { path: "/workspace/id_ed25519" } },
  { name: "manual Read id_ed25519.pub approves (exempt)", mode: "manual", toolName: "Read", args: { path: "/workspace/id_ed25519.pub" } },
  // Git control path access asks.
  { name: "manual Read .git/config asks (control path)", mode: "manual", toolName: "Read", args: { path: "/workspace/.git/config" } },
];

describe("permission chain TS↔Rust differential", () => {
  let disposables: DisposableStore;
  let ix: TestInstantiationService;
  let mode: PermissionMode;
  let rules: PermissionRule[];
  let sessionApprovalRulePatterns: string[];
  let workspace: ReturnType<typeof workspaceStub>;

  beforeEach(() => {
    disposables = new DisposableStore();
    mode = "manual";
    rules = [];
    sessionApprovalRulePatterns = [];
    workspace = workspaceStub("/workspace");
    ix = createServices(disposables, {
      additionalServices: (reg) => {
        reg.defineInstance(IAgentPermissionModeService, stubPermissionModeService(() => mode));
        reg.defineInstance(IAgentScopeContext, makeAgentScopeContext({ agentId: "main", agentScope: "" }));
        reg.definePartialInstance(
          IAgentPermissionRulesService,
          permissionRulesStub({
            rules: () => rules,
            sessionApprovalRulePatterns: () => sessionApprovalRulePatterns,
          }),
        );
        reg.defineInstance(ISessionWorkspaceContext, workspace);
        reg.defineInstance(IHostEnvironment, kaosStub());
        reg.defineInstance(ITelemetryService, recordingTelemetry([]));
        reg.define(IAgentPermissionPolicyService, AgentPermissionPolicyService);
      },
      strict: true,
    });
  });

  afterEach(() => {
    disposables.dispose();
  });

  async function tsEvaluation(
    input: PolicyContextInput,
  ): Promise<{ kind: string; reason?: string } | undefined> {
    const svc = ix.get(IAgentPermissionPolicyService);
    const evaluation = await svc.evaluate(policyContext(input));
    if (evaluation === undefined) return undefined;
    const result = evaluation.result as { kind: string; message?: string };
    return {
      kind: result.kind,
      reason: result.kind === "deny" ? result.message : undefined,
    };
  }

  function rustEvaluation(c: DifferentialCase): { decision: string; reason?: string } {
    const input = {
      mode: c.mode,
      toolName: c.toolName,
      args: c.args,
      rules: c.rules ?? [],
      sessionApprovedPatterns: c.sessionApprovalRulePatterns ?? [],
      matchArg: c.toolName === "Bash" ? stringArg(c.args, "command") : undefined,
      cwd: c.workspaceDir ?? "/workspace",
      paths: c.toolName === "Bash" ? [] : pathArgPaths(c.args),
    };
    return JSON.parse(evaluatePolicy(JSON.stringify(input))) as { decision: string; reason?: string };
  }

  for (const c of CASES) {
    it(c.name, async () => {
      mode = c.mode;
      rules = [...(c.rules ?? [])];
      sessionApprovalRulePatterns = [...(c.sessionApprovalRulePatterns ?? [])];
      if (c.workspaceDir !== undefined) workspace.setWorkDir(c.workspaceDir);

      const ts = await tsEvaluation({ toolName: c.toolName, args: c.args });
      const rust = rustEvaluation(c);

      expect(
        rust,
        `Rust decision must match TS for ${c.name} (TS: ${JSON.stringify(ts)})`,
      ).toEqual({
        decision: ts?.kind ?? "ask",
        reason: ts?.reason,
      });
    });
  }

  it("git-cwd write approval matches TS on a real git work tree", async () => {
    const workspaceDir = await mkdtemp(join(tmpdir(), "dimi-perm-diff-"));
    try {
      await mkdir(join(workspaceDir, ".git"), { recursive: true });
      mode = "manual";
      workspace.setWorkDir(workspaceDir);
      const c: DifferentialCase = {
        name: "git-cwd write approves in-repo edit",
        mode: "manual",
        toolName: "Write",
        args: { path: join(workspaceDir, "src/a.ts"), content: "x" },
        workspaceDir,
      };
      const ts = await tsEvaluation({ toolName: "Write", args: { path: join(workspaceDir, "src/a.ts"), content: "x" } });
      const rust = rustEvaluation(c);
      expect(rust).toEqual({ decision: ts?.kind ?? "ask", reason: ts?.reason });

      // An out-of-workspace write still asks on both sides.
      const outside = join(tmpdir(), "dimi-perm-outside.ts");
      const outsideTs = await tsEvaluation({
        toolName: "Write",
        args: { path: outside, content: "x" },
        accesses: ToolAccesses.writeFile(outside),
      });
      const outsideRust = rustEvaluation({
        name: "outside write asks",
        mode: "manual",
        toolName: "Write",
        args: { path: outside, content: "x" },
        workspaceDir,
      });
      expect(outsideRust).toEqual({ decision: outsideTs?.kind ?? "ask", reason: outsideTs?.reason });
    } finally {
      await rm(workspaceDir, { recursive: true, force: true });
    }
  });
});

interface PolicyContextInput {
  readonly id?: string;
  readonly toolName: string;
  readonly args: Record<string, unknown>;
  readonly accesses?: ToolAccessList;
}

function policyContext(input: PolicyContextInput): ResolvedToolExecutionHookContext {
  const toolCall = toolCallFor(input.id ?? `call_${input.toolName}`, input.toolName, input.args);
  const subject = ruleSubject(input.toolName, input.args);
  return {
    turnId: 0,
    signal,
    toolCall,
    toolCalls: [toolCall],
    args: input.args,
    execution: {
      description: `Approve ${input.toolName}`,
      display: { kind: "generic", summary: `Approve ${input.toolName}`, detail: input.args },
      accesses: input.accesses ?? accesses(input.toolName, input.args),
      approvalRule:
        subject === undefined ? input.toolName : literalRulePattern(input.toolName, subject),
      matchesRule:
        subject === undefined
          ? undefined
          : (ruleArgs) => matchesRuleSubject(input.toolName, ruleArgs, subject),
      execute: async () => ({ output: "" }),
    },
  };
}

function toolCallFor(id: string, name: string, args: Record<string, unknown>): ToolCall {
  return { type: "function", id, name, arguments: JSON.stringify(args) };
}

function ruleSubject(toolName: string, args: Record<string, unknown>): string | undefined {
  switch (toolName) {
    case "Bash":
      return stringArg(args, "command");
    case "Read":
    case "ReadMediaFile":
    case "Write":
    case "Edit":
      return stringArg(args, "path");
    case "Grep":
    case "Glob":
      return stringArg(args, "pattern");
    default:
      return undefined;
  }
}

function matchesRuleSubject(toolName: string, ruleArgs: string, subject: string): boolean {
  switch (toolName) {
    case "Read":
    case "ReadMediaFile":
    case "Write":
    case "Edit":
      return matchesPathRuleSubject(ruleArgs, subject, { cwd: "/workspace", pathClass: "posix" });
    default:
      return matchesGlobRuleSubject(ruleArgs, subject);
  }
}

function accesses(toolName: string, args: Record<string, unknown>): ToolAccessList {
  const path = stringArg(args, "path");
  switch (toolName) {
    case "Read":
    case "ReadMediaFile":
      return path.length > 0 ? ToolAccesses.readFile(path) : ToolAccesses.none();
    case "Write":
      return path.length > 0 ? ToolAccesses.writeFile(path) : ToolAccesses.none();
    case "Edit":
      return path.length > 0 ? ToolAccesses.readWriteFile(path) : ToolAccesses.none();
    case "Grep":
    case "Glob":
      return path.length > 0 ? ToolAccesses.searchTree(path) : ToolAccesses.none();
    default:
      return ToolAccesses.none();
  }
}

function pathArgPaths(args: Record<string, unknown>): string[] {
  const path = stringArg(args, "path");
  return path.length > 0 ? [path] : [];
}

function stringArg(args: Record<string, unknown>, key: string, fallback = ""): string {
  const value = args[key];
  return typeof value === "string" ? value : fallback;
}

function workspaceStub(initialWorkDir: string): ISessionWorkspaceContext {
  let workDir = initialWorkDir;
  let additionalDirs: string[] = [];
  return {
    _serviceBrand: undefined,
    get workDir() {
      return workDir;
    },
    get additionalDirs() {
      return additionalDirs;
    },
    setWorkDir: (nextWorkDir) => {
      workDir = nextWorkDir;
    },
    setAdditionalDirs: (dirs) => {
      additionalDirs = [...dirs];
    },
    resolve: (path) => path,
    isWithin: () => true,
    assertAllowed: (path) => path,
    addAdditionalDir: (dir) => {
      if (!additionalDirs.includes(dir)) additionalDirs = [...additionalDirs, dir];
    },
    removeAdditionalDir: (dir) => {
      additionalDirs = additionalDirs.filter((candidate) => candidate !== dir);
    },
  };
}

function kaosStub(pathClass: HostEnvironmentService["pathClass"] = "posix"): HostEnvironmentService {
  return {
    _serviceBrand: undefined,
    osKind: "Linux",
    osArch: "x86_64",
    osVersion: "test",
    shellName: "bash",
    shellPath: "/bin/bash",
    pathClass,
    homeDir: "/home/test",
    ready: Promise.resolve(),
  };
}

interface MutablePermissionRulesStubOptions {
  readonly rules?: () => readonly PermissionRule[];
  readonly sessionApprovalRulePatterns?: () => readonly string[];
}

function permissionRulesStub(
  options: MutablePermissionRulesStubOptions = {},
): Partial<PermissionRulesServiceContract> {
  const rules = options.rules ?? (() => []);
  const sessionApprovalRulePatterns = options.sessionApprovalRulePatterns ?? (() => []);
  return {
    get rules() {
      return rules();
    },
    get sessionApprovalRulePatterns() {
      return sessionApprovalRulePatterns();
    },
    addRules: () => {},
    recordApprovalResult: () => {},
  };
}
