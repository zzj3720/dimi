import { execSync } from "node:child_process";

import type { createDimiDeviceId as createDimiDeviceIdFn } from "@dimi-agent/dimi-oauth";
import { afterEach, describe, expect, it, vi } from "vitest";

import { runShell } from "#/cli/run-shell";

import { captureProcessWrite, ExitCalled, mockProcessExit } from "../helpers/process";

type CreateDimiDeviceId = typeof createDimiDeviceIdFn;

const mocks = vi.hoisted(() => {
  type TuiConfigFallback = {
    theme: "dark" | "light" | "auto";
    editorCommand: string | null;
    notifications: { enabled: boolean; condition: "unfocused" | "always" };
  };

  class TuiConfigParseError extends Error {
    readonly fallback: TuiConfigFallback;

    constructor(fallback: TuiConfigFallback) {
      super("Invalid TUI config in ~/.dimi/tui.toml; using defaults.");
      this.fallback = fallback;
    }
  }

  const lifecycleTrack = vi.fn();

  return {
    loadTuiConfig: vi.fn(),
    detectTerminalTheme: vi.fn(),
    dimiHarnessConstructor: vi.fn(),
    harnessEnsureConfigFile: vi.fn(),
    harnessGetConfig: vi.fn(async () => ({
      providers: {},
      defaultModel: "k2",
      telemetry: true,
    })),
    harnessGetConfigDiagnostics: vi.fn(async () => ({ warnings: [] as readonly string[] })),
    harnessGetCachedAccessToken: vi.fn(),
    harnessClose: vi.fn(),
    harnessTrack: vi.fn(),
    dimiTuiConstructor: vi.fn(),
    tuiStart: vi.fn(),
    tuiGetStartupMcpMs: vi.fn(async () => 0),
    tuiGetCurrentSessionId: vi.fn(() => ""),
    tuiHasSessionContent: vi.fn(() => false),
    createDimiDeviceId: vi.fn<CreateDimiDeviceId>(() => "device-1"),
    initializeTelemetry: vi.fn(),
    setCrashPhase: vi.fn(),
    shutdownTelemetry: vi.fn(),
    telemetryTrack: vi.fn(),
    setTelemetryContext: vi.fn(),
    lifecycleTrack,
    withTelemetryContext: vi.fn(() => ({
      track: lifecycleTrack,
    })),
    resolveDimiHome: vi.fn((homeDir?: string) => homeDir ?? "/tmp/dimi-test-home"),
    flushDiagnosticLogsSync: vi.fn(),
    harnessCreatesDeviceIdOnConstruction: false,
    execSync: vi.fn(),
    TuiConfigParseError,
  };
});

vi.mock("@dimi-agent/dimi-sdk", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@dimi-agent/dimi-sdk")>();
  const makeHarnessStub = (args: unknown[]) => {
    const options = args[0] as { readonly homeDir?: string } | undefined;
    const homeDir = options?.homeDir ?? "/tmp/dimi-test-home";
    return {
      homeDir,
      auth: {
        getCachedAccessToken: mocks.harnessGetCachedAccessToken,
      },
      ensureConfigFile: mocks.harnessEnsureConfigFile,
      getConfig: mocks.harnessGetConfig,
      getConfigDiagnostics: mocks.harnessGetConfigDiagnostics,
      close: mocks.harnessClose,
      track: mocks.harnessTrack,
    };
  };
  return {
    ...actual,
    resolveDimiHome: mocks.resolveDimiHome,
    flushDiagnosticLogsSync: mocks.flushDiagnosticLogsSync,
    createDimiHarness: (...args: unknown[]) => {
      const options = args[0] as { readonly homeDir?: string } | undefined;
      const homeDir = options?.homeDir ?? "/tmp/dimi-test-home";
      if (mocks.harnessCreatesDeviceIdOnConstruction) {
        mocks.createDimiDeviceId(homeDir);
      }
      mocks.dimiHarnessConstructor(...args);
      return makeHarnessStub(args);
    },
  };
});

vi.mock("@dimi-agent/dimi-oauth", async () => {
  const actual = await vi.importActual<typeof import("@dimi-agent/dimi-oauth")>(
    "@dimi-agent/dimi-oauth",
  );
  return {
    ...actual,
    createDimiDeviceId: mocks.createDimiDeviceId,
    DIMI_CODE_PROVIDER_NAME: "dimi",
  };
});

vi.mock("@dimi-agent/dimi-telemetry", () => ({
  initializeTelemetry: mocks.initializeTelemetry,
  setCrashPhase: mocks.setCrashPhase,
  shutdownTelemetry: mocks.shutdownTelemetry,
  track: mocks.telemetryTrack,
  setTelemetryContext: mocks.setTelemetryContext,
  withTelemetryContext: mocks.withTelemetryContext,
}));

vi.mock("../../src/tui/config", () => ({
  loadTuiConfig: mocks.loadTuiConfig,
  TuiConfigParseError: mocks.TuiConfigParseError,
}));

vi.mock("../../src/tui/index", () => ({
  DimiTUI: class {
    onExit?: () => Promise<void>;

    constructor(...args: unknown[]) {
      mocks.dimiTuiConstructor(this, ...args);
    }

    start = mocks.tuiStart;
    getStartupMcpMs = mocks.tuiGetStartupMcpMs;
    getCurrentSessionId = mocks.tuiGetCurrentSessionId;
    hasSessionContent = mocks.tuiHasSessionContent;
  },
}));

vi.mock("../../src/tui/theme/detect", () => ({
  detectTerminalTheme: mocks.detectTerminalTheme,
}));

vi.mock("node:child_process", () => ({
  execSync: mocks.execSync,
}));

describe("runShell", () => {
  afterEach(() => {
    vi.clearAllMocks();
    mocks.harnessGetConfig.mockResolvedValue({
      providers: {},
      defaultModel: "k2",
      telemetry: true,
    });
    mocks.tuiGetStartupMcpMs.mockResolvedValue(0);
    mocks.tuiGetCurrentSessionId.mockReturnValue("");
    mocks.tuiHasSessionContent.mockReturnValue(false);
    mocks.createDimiDeviceId.mockImplementation(() => "device-1");
    mocks.resolveDimiHome.mockImplementation(
      (homeDir?: string) => homeDir ?? "/tmp/dimi-test-home",
    );
    mocks.harnessCreatesDeviceIdOnConstruction = false;
  });

  const minimalCliOptions = {
    session: undefined,
    continue: false,
    yolo: false,
    auto: false,
    plan: false,
    model: undefined,
    outputFormat: undefined,
    prompt: undefined,
    skillsDirs: [],
      legacy: false,
    agent: undefined,
    agentFiles: [],
  };

  it("constructs DimiHarness and DimiTUI with startup input", async () => {
    mocks.loadTuiConfig.mockResolvedValue({
      theme: "dark",
      editorCommand: null,
      notifications: { enabled: true, condition: "unfocused" },
    });
    mocks.tuiStart.mockResolvedValue(undefined);
    mocks.tuiGetStartupMcpMs.mockResolvedValue(47);
    mocks.tuiGetCurrentSessionId.mockReturnValue("ses-startup");

    const cliOptions = {
      session: undefined,
      continue: false,
      yolo: true,
      auto: false,
      plan: true,
      model: undefined,
      outputFormat: undefined,
      prompt: undefined,
      skillsDirs: [],
      legacy: false,
      agent: undefined,
      agentFiles: [],
      addDirs: ["../shared", "/tmp/extra"],
    };

    await runShell(cliOptions, "1.2.3-test");

    expect(mocks.dimiHarnessConstructor).toHaveBeenCalledWith(
      expect.objectContaining({
        identity: expect.objectContaining({
          userAgentProduct: "dimi-cli",
          version: "1.2.3-test",
        }),
        sessionStartedProperties: { yolo: true, auto: false, plan: true, afk: false },
      }),
    );
    expect(mocks.harnessEnsureConfigFile).toHaveBeenCalledOnce();
    expect(mocks.harnessEnsureConfigFile.mock.invocationCallOrder[0]).toBeLessThan(
      mocks.harnessGetConfig.mock.invocationCallOrder[0]!,
    );
    expect(execSync).toHaveBeenCalledWith("stty -ixon", { stdio: ["inherit", "ignore", "ignore"] });
    expect(mocks.dimiTuiConstructor).toHaveBeenCalledTimes(1);
    expect(mocks.createDimiDeviceId).toHaveBeenCalledWith(
      "/tmp/dimi-test-home",
      expect.any(Object),
    );
    expect(mocks.initializeTelemetry).toHaveBeenCalledWith({
      homeDir: "/tmp/dimi-test-home",
      deviceId: "device-1",
      enabled: true,
      appName: "dimi-cli",
      version: "1.2.3-test",
      uiMode: "shell",
      model: "k2",
      sessionId: undefined,
      getAccessToken: expect.any(Function),
    });
    expect(mocks.setCrashPhase).toHaveBeenCalledWith("runtime");

    const [, harness, startupInput] = mocks.dimiTuiConstructor.mock.calls[0]!;
    expect(harness).toBeTypeOf("object");
    expect(startupInput).toMatchObject({
      cliOptions,
      additionalDirs: ["../shared", "/tmp/extra"],
      tuiConfig: {
        theme: "dark",
        editorCommand: null,
        notifications: { enabled: true, condition: "unfocused" },
      },
      version: "1.2.3-test",
      workDir: process.cwd(),
    });
    expect(mocks.tuiStart).toHaveBeenCalledOnce();
    expect(mocks.withTelemetryContext).toHaveBeenCalledWith({ sessionId: "ses-startup" });
    expect(mocks.lifecycleTrack).toHaveBeenCalledWith("startup_perf", {
      duration_ms: expect.any(Number),
      config_ms: expect.any(Number),
      init_ms: expect.any(Number),
      mcp_ms: 47,
    });
  });

  it("resolves the --agent profile into the TUI startup input", async () => {
    mocks.loadTuiConfig.mockResolvedValue({
      theme: "dark",
      editorCommand: null,
      notifications: { enabled: true, condition: "unfocused" },
    });
    mocks.tuiStart.mockResolvedValue(undefined);

    await runShell(
      {
        session: undefined,
        continue: false,
        yolo: false,
        auto: false,
        plan: false,
        model: undefined,
        outputFormat: undefined,
        prompt: undefined,
        skillsDirs: [],
      legacy: false,
        agent: "reviewer",
        agentFiles: [],
      },
      "1.2.3-test",
    );

    const [, , startupInput] = mocks.dimiTuiConstructor.mock.calls[0]!;
    expect(startupInput).toMatchObject({ agentProfile: "reviewer" });
  });

  it("forwards skillsDirs from CLI options to the harness", async () => {
    mocks.loadTuiConfig.mockResolvedValue({
      theme: "dark",
      editorCommand: null,
      notifications: { enabled: true, condition: "unfocused" },
    });
    mocks.tuiStart.mockResolvedValue(undefined);

    await runShell(
      {
        session: undefined,
        continue: false,
        yolo: false,
        auto: false,
        plan: false,
        model: undefined,
        outputFormat: undefined,
        prompt: undefined,
        skillsDirs: ["/skills"],
      legacy: false,
        agent: undefined,
        agentFiles: [],
      },
      "1.2.3-test",
    );

    expect(mocks.dimiHarnessConstructor).toHaveBeenCalledWith(
      expect.objectContaining({ skillDirs: ["/skills"] }),
    );
  });

  it("tracks first launch when device id creation reports first launch", async () => {
    mocks.loadTuiConfig.mockResolvedValue({
      theme: "dark",
      editorCommand: null,
      notifications: { enabled: true, condition: "unfocused" },
    });
    mocks.tuiStart.mockResolvedValue(undefined);
    mocks.createDimiDeviceId.mockImplementationOnce((homeDir, options) => {
      const deviceId = `device-for-${homeDir}`;
      options?.onFirstLaunch?.(deviceId);
      return deviceId;
    });

    await runShell(
      {
        session: undefined,
        continue: false,
        yolo: false,
        auto: false,
        plan: false,
        model: undefined,
        outputFormat: undefined,
        prompt: undefined,
        skillsDirs: [],
      legacy: false,
        agent: undefined,
        agentFiles: [],
      },
      "1.2.3-test",
    );

    expect(mocks.createDimiDeviceId).toHaveBeenCalledWith(
      "/tmp/dimi-test-home",
      expect.objectContaining({ onFirstLaunch: expect.any(Function) }),
    );
    expect(mocks.harnessTrack).toHaveBeenCalledWith("first_launch");
  });

  it("registers first launch before harness construction can create the device id", async () => {
    mocks.loadTuiConfig.mockResolvedValue({
      theme: "dark",
      editorCommand: null,
      notifications: { enabled: true, condition: "unfocused" },
    });
    mocks.tuiStart.mockResolvedValue(undefined);
    mocks.harnessCreatesDeviceIdOnConstruction = true;
    const createdHomes = new Set<string>();
    mocks.createDimiDeviceId.mockImplementation((homeDir, options) => {
      const deviceId = `device-for-${homeDir}`;
      if (!createdHomes.has(homeDir)) {
        createdHomes.add(homeDir);
        options?.onFirstLaunch?.(deviceId);
      }
      return deviceId;
    });

    await runShell(
      {
        session: undefined,
        continue: false,
        yolo: false,
        auto: false,
        plan: false,
        model: undefined,
        outputFormat: undefined,
        prompt: undefined,
        skillsDirs: [],
      legacy: false,
        agent: undefined,
        agentFiles: [],
      },
      "1.2.3-test",
    );

    expect(mocks.createDimiDeviceId).toHaveBeenNthCalledWith(
      1,
      "/tmp/dimi-test-home",
      expect.objectContaining({ onFirstLaunch: expect.any(Function) }),
    );
    expect(mocks.createDimiDeviceId.mock.invocationCallOrder[0]).toBeLessThan(
      mocks.dimiHarnessConstructor.mock.invocationCallOrder[0]!,
    );
    expect(mocks.dimiHarnessConstructor).toHaveBeenCalledWith(
      expect.objectContaining({ homeDir: "/tmp/dimi-test-home" }),
    );
    expect(mocks.harnessTrack).toHaveBeenCalledWith("first_launch");
  });

  it("binds startup_perf to the session captured before MCP metrics resolve", async () => {
    mocks.loadTuiConfig.mockResolvedValue({
      theme: "dark",
      editorCommand: null,
      notifications: { enabled: true, condition: "unfocused" },
    });
    mocks.tuiStart.mockResolvedValue(undefined);
    let currentSessionId = "ses-startup";
    mocks.tuiGetCurrentSessionId.mockImplementation(() => currentSessionId);
    mocks.tuiGetStartupMcpMs.mockImplementation(async () => {
      currentSessionId = "ses-later";
      return 47;
    });

    await runShell(
      {
        session: undefined,
        continue: false,
        yolo: false,
        auto: false,
        plan: false,
        model: undefined,
        outputFormat: undefined,
        prompt: undefined,
        skillsDirs: [],
      legacy: false,
        agent: undefined,
        agentFiles: [],
      },
      "1.2.3-test",
    );

    expect(mocks.withTelemetryContext).toHaveBeenCalledWith({ sessionId: "ses-startup" });
    expect(mocks.withTelemetryContext).not.toHaveBeenCalledWith({ sessionId: "ses-later" });
    expect(mocks.lifecycleTrack).toHaveBeenCalledWith("startup_perf", {
      duration_ms: expect.any(Number),
      config_ms: expect.any(Number),
      init_ms: expect.any(Number),
      mcp_ms: 47,
    });
  });

  it("detects auto theme and forwards config parse warnings as startup notice", async () => {
    mocks.loadTuiConfig.mockRejectedValue(
      new mocks.TuiConfigParseError({
        theme: "auto",
        editorCommand: "vim",
        notifications: { enabled: true, condition: "always" },
      }),
    );
    mocks.detectTerminalTheme.mockResolvedValue("light");
    mocks.tuiStart.mockResolvedValue(undefined);

    await runShell(
      {
        session: "",
        continue: false,
        yolo: false,
        auto: false,
        plan: false,
        model: undefined,
        outputFormat: undefined,
        prompt: undefined,
        skillsDirs: [],
      legacy: false,
        agent: undefined,
        agentFiles: [],
      },
      "1.2.3-test",
    );

    expect(mocks.detectTerminalTheme).toHaveBeenCalledOnce();
    const [, , startupInput] = mocks.dimiTuiConstructor.mock.calls[0]!;
    expect(startupInput).toMatchObject({
      startupNotice: "Invalid TUI config in ~/.dimi/tui.toml; using defaults.",
      tuiConfig: {
        theme: "auto",
        editorCommand: "vim",
        notifications: { enabled: true, condition: "always" },
      },
    });
  });

  it("forwards config.toml diagnostics as startup notices", async () => {
    mocks.loadTuiConfig.mockResolvedValue({
      theme: "dark",
      editorCommand: null,
      notifications: { enabled: true, condition: "unfocused" },
    });
    mocks.harnessGetConfigDiagnostics.mockResolvedValue({
      warnings: ["Ignored invalid config in config.toml: loop_control."],
    });
    mocks.tuiStart.mockResolvedValue(undefined);

    await runShell(
      {
        session: "",
        continue: false,
        yolo: false,
        auto: false,
        plan: false,
        model: undefined,
        outputFormat: undefined,
        prompt: undefined,
        skillsDirs: [],
      legacy: false,
        agent: undefined,
        agentFiles: [],
      },
      "1.2.3-test",
    );

    const [, , startupInput] = mocks.dimiTuiConstructor.mock.calls[0]!;
    expect(startupInput).toMatchObject({
      startupNotice: "Ignored invalid config in config.toml: loop_control.",
    });
  });

  it("flushes diagnostic logs synchronously before exiting on a runtime crash", async () => {
    mocks.loadTuiConfig.mockResolvedValue({
      theme: "dark",
      editorCommand: null,
      notifications: { enabled: true, condition: "unfocused" },
    });
    mocks.tuiStart.mockResolvedValue(undefined);

    const processOnSpy = vi.spyOn(process, "on");
    const stdout = captureProcessWrite("stdout");
    const exitSpy = mockProcessExit();

    try {
      await runShell(
        {
          session: undefined,
          continue: false,
          yolo: false,
          auto: false,
          plan: false,
          model: undefined,
          outputFormat: undefined,
          prompt: undefined,
          skillsDirs: [],
      legacy: false,
          agent: undefined,
          agentFiles: [],
        },
        "1.2.3-test",
      );

      const handler = processOnSpy.mock.calls.find(
        ([event]) => event === "uncaughtException",
      )?.[1] as ((error: unknown) => void) | undefined;
      expect(handler).toBeDefined();

      // The async log sink cannot flush before process.exit() runs, so the
      // crash handler must force a synchronous flush or the crash reason is
      // lost (regression: uncaughtException logs never reached disk).
      expect(() => handler?.(new Error("boom"))).toThrow(ExitCalled);
      expect(mocks.flushDiagnosticLogsSync).toHaveBeenCalledOnce();
      expect(exitSpy).toHaveBeenCalledWith(1);
      expect(mocks.flushDiagnosticLogsSync.mock.invocationCallOrder[0]!).toBeLessThan(
        exitSpy.mock.invocationCallOrder[0]!,
      );
    } finally {
      processOnSpy.mockRestore();
      exitSpy.mockRestore();
      stdout.restore();
    }
  });

  it("flushes diagnostic logs synchronously before exiting on an unhandled rejection", async () => {
    mocks.loadTuiConfig.mockResolvedValue({
      theme: "dark",
      editorCommand: null,
      notifications: { enabled: true, condition: "unfocused" },
    });
    mocks.tuiStart.mockResolvedValue(undefined);

    const processOnSpy = vi.spyOn(process, "on");
    const stdout = captureProcessWrite("stdout");
    const exitSpy = mockProcessExit();

    try {
      await runShell(
        {
          session: undefined,
          continue: false,
          yolo: false,
          auto: false,
          plan: false,
          model: undefined,
          outputFormat: undefined,
          prompt: undefined,
          skillsDirs: [],
      legacy: false,
          agent: undefined,
          agentFiles: [],
        },
        "1.2.3-test",
      );

      const handler = processOnSpy.mock.calls.find(
        ([event]) => event === "unhandledRejection",
      )?.[1] as ((reason: unknown) => void) | undefined;
      expect(handler).toBeDefined();

      expect(() => handler?.(new Error("boom"))).toThrow(ExitCalled);
      expect(mocks.flushDiagnosticLogsSync).toHaveBeenCalledOnce();
      expect(exitSpy).toHaveBeenCalledWith(1);
      expect(mocks.flushDiagnosticLogsSync.mock.invocationCallOrder[0]!).toBeLessThan(
        exitSpy.mock.invocationCallOrder[0]!,
      );
    } finally {
      processOnSpy.mockRestore();
      exitSpy.mockRestore();
      stdout.restore();
    }
  });

  it("closes the harness when TUI startup fails", async () => {
    mocks.loadTuiConfig.mockResolvedValue({
      theme: "dark",
      editorCommand: null,
      notifications: { enabled: true, condition: "unfocused" },
    });
    mocks.tuiStart.mockRejectedValue(new Error("boom"));

    await expect(
      runShell(
        {
          session: undefined,
          continue: false,
          yolo: false,
          auto: false,
          plan: false,
          model: undefined,
          outputFormat: undefined,
          prompt: undefined,
          skillsDirs: [],
      legacy: false,
          agent: undefined,
          agentFiles: [],
        },
        "1.2.3-test",
      ),
    ).rejects.toThrow("boom");

    expect(mocks.setCrashPhase).toHaveBeenCalledWith("shutdown");
    expect(mocks.harnessTrack).toHaveBeenCalledWith("exit", { duration_ms: expect.any(Number) });
    expect(mocks.shutdownTelemetry).toHaveBeenCalledOnce();
    expect(mocks.harnessClose).toHaveBeenCalledOnce();
  });

  it("tracks exit and prints resume instructions from the TUI exit handler", async () => {
    mocks.loadTuiConfig.mockResolvedValue({
      theme: "dark",
      editorCommand: null,
      notifications: { enabled: true, condition: "unfocused" },
    });
    mocks.tuiStart.mockResolvedValue(undefined);
    mocks.tuiGetCurrentSessionId.mockReturnValue("ses-1");
    mocks.tuiHasSessionContent.mockReturnValue(true);

    const stdout = captureProcessWrite("stdout");
    const stderr = captureProcessWrite("stderr");
    const exitSpy = mockProcessExit();

    try {
      await runShell(
        {
          session: undefined,
          continue: false,
          yolo: false,
          auto: false,
          plan: false,
          model: undefined,
          outputFormat: undefined,
          prompt: undefined,
          skillsDirs: [],
      legacy: false,
          agent: undefined,
          agentFiles: [],
        },
        "1.2.3-test",
      );
      const [tui] = mocks.dimiTuiConstructor.mock.calls[0]!;
      mocks.harnessTrack.mockClear();
      mocks.lifecycleTrack.mockClear();
      mocks.withTelemetryContext.mockClear();

      await expect((tui as { onExit: () => Promise<void> }).onExit()).rejects.toBeInstanceOf(
        ExitCalled,
      );

      expect(mocks.setCrashPhase).toHaveBeenCalledWith("shutdown");
      expect(mocks.withTelemetryContext).toHaveBeenCalledWith({ sessionId: "ses-1" });
      expect(mocks.lifecycleTrack).toHaveBeenCalledWith("exit", {
        duration_ms: expect.any(Number),
      });
      expect(mocks.harnessTrack).not.toHaveBeenCalledWith("exit", expect.anything());
      expect(mocks.shutdownTelemetry).toHaveBeenCalledOnce();
      expect(stdout.text()).toBe(" Bye!\n");
      expect(stderr.text()).toContain(" To resume this session: dimi -r ses-1");
    } finally {
      exitSpy.mockRestore();
      stdout.restore();
      stderr.restore();
    }
  });

  it("prints the opened web URL from the TUI exit handler when set", async () => {
    mocks.loadTuiConfig.mockResolvedValue({
      theme: "dark",
      editorCommand: null,
      notifications: { enabled: true, condition: "unfocused" },
    });
    mocks.tuiStart.mockResolvedValue(undefined);
    mocks.tuiGetCurrentSessionId.mockReturnValue("ses-1");
    mocks.tuiHasSessionContent.mockReturnValue(true);

    const stdout = captureProcessWrite("stdout");
    const stderr = captureProcessWrite("stderr");
    const exitSpy = mockProcessExit();

    try {
      await runShell(
        {
          session: undefined,
          continue: false,
          yolo: false,
          auto: false,
          plan: false,
          model: undefined,
          outputFormat: undefined,
          prompt: undefined,
          skillsDirs: [],
      legacy: false,
          agent: undefined,
          agentFiles: [],
        },
        "1.2.3-test",
      );
      const [tui] = mocks.dimiTuiConstructor.mock.calls[0]!;
      const openedUrl = "http://127.0.0.1:58627/sessions/ses-1#token=tok-1";
      (tui as { exitOpenUrl?: string }).exitOpenUrl = openedUrl;

      await expect((tui as { onExit: () => Promise<void> }).onExit()).rejects.toBeInstanceOf(
        ExitCalled,
      );

      expect(stderr.text()).toContain(" To resume this session: dimi -r ses-1");
      expect(stderr.text()).toContain("open ");
      expect(stderr.text()).toContain(openedUrl);
    } finally {
      exitSpy.mockRestore();
      stdout.restore();
      stderr.restore();
    }
  });
});
