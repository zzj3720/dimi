/**
 * `dimi login`
 *
 * Verifies that the login sub-command is registered on the program and
 * that the action resolves a provider method, drives `harness.auth.login`,
 * prints device-code state to stderr, and exits with the right code.
 */

import { Command } from "commander";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createDimiHarness } from "@dimi-agent/dimi-sdk";

import { registerLoginCommand } from "#/cli/sub/login";
import { openUrl } from "#/utils/open-url";

const mockLogin = vi.fn();
const mockProviders = vi.fn();

vi.mock("@dimi-agent/dimi-sdk", async () => {
  const actual = await vi.importActual<typeof import("@dimi-agent/dimi-sdk")>(
    "@dimi-agent/dimi-sdk",
  );
  return {
    ...actual,
    createDimiHarness: vi.fn(() => ({
      auth: {
        login: mockLogin,
        providers: mockProviders,
      },
      close: vi.fn(),
    })),
  };
});

vi.mock("#/utils/open-url", () => ({ openUrl: vi.fn() }));

class ExitCalled extends Error {
  constructor(public code: number | string | null | undefined) {
    super(`process.exit(${String(code)})`);
  }
}

describe("dimi login", () => {
  let exitSpy: ReturnType<typeof vi.spyOn>;
  let stderrSpy: ReturnType<typeof vi.spyOn>;

  beforeEach(() => {
    mockLogin.mockReset();
    mockProviders.mockReset();
    mockProviders.mockResolvedValue([
      {
        id: "kimi-coding",
        name: "Dimi",
        configured: false,
        methods: [{ type: "oauth" }],
      },
    ]);
    vi.mocked(openUrl).mockReset();
    vi.mocked(createDimiHarness).mockClear();
    exitSpy = vi.spyOn(process, "exit").mockImplementation(((code?: number | string | null) => {
      throw new ExitCalled(code);
    }) as never);
    stderrSpy = vi.spyOn(process.stderr, "write").mockImplementation(() => true);
  });

  afterEach(() => {
    exitSpy.mockRestore();
    stderrSpy.mockRestore();
  });

  it("registers a `login` subcommand on the program", () => {
    const program = new Command("dimi");
    registerLoginCommand(program);

    const login = program.commands.find((c) => c.name() === "login");
    expect(login).toBeDefined();
    expect(login?.description()).toMatch(/[Cc]onnect/);
  });

  it("invokes harness.auth.login and exits 0 on success", async () => {
    mockLogin.mockResolvedValue({ provider: "kimi-coding", credentialType: "oauth", models: [] });

    const program = new Command("dimi").exitOverride();
    registerLoginCommand(program);

    await expect(program.parseAsync(["node", "dimi", "login"])).rejects.toThrow(ExitCalled);

    expect(mockLogin).toHaveBeenCalledTimes(1);
    expect(mockLogin).toHaveBeenCalledWith(
      "kimi-coding",
      "oauth",
      expect.objectContaining({
        signal: expect.any(AbortSignal),
        notify: expect.any(Function),
      }),
    );
    expect(exitSpy).toHaveBeenCalledWith(0);
  });

  it("prints device code prompt to stderr", async () => {
    mockLogin.mockImplementation(
      async (
        _providerName: string,
        _method: string,
        options: {
          notify?: (data: {
            type: "device_code";
            userCode: string;
            verificationUri: string;
          }) => void;
        },
      ) => {
        options.notify?.({
          type: "device_code",
          userCode: "ABCD-EFGH",
          verificationUri: "https://example.com/v",
        });
        return { provider: "kimi-coding", credentialType: "oauth", models: [] };
      },
    );

    const program = new Command("dimi").exitOverride();
    registerLoginCommand(program);

    await expect(program.parseAsync(["node", "dimi", "login"])).rejects.toThrow(ExitCalled);

    const writtenChunks = stderrSpy.mock.calls.map((call: unknown[]) => String(call[0]));
    expect(writtenChunks.some((chunk: string) => chunk.includes("ABCD-EFGH"))).toBe(true);
    expect(writtenChunks.some((chunk: string) => chunk.includes("https://example.com/v"))).toBe(
      true,
    );
    expect(openUrl).toHaveBeenCalledWith("https://example.com/v");
    expect(exitSpy).toHaveBeenCalledWith(0);
  });

  it("still prints device code prompt when opening the browser fails", async () => {
    vi.mocked(openUrl).mockImplementation(() => {
      throw new Error("no browser");
    });
    mockLogin.mockImplementation(
      async (
        _providerName: string,
        _method: string,
        options: {
          notify?: (data: {
            type: "device_code";
            userCode: string;
            verificationUri: string;
          }) => void;
        },
      ) => {
        options.notify?.({
          type: "device_code",
          userCode: "ABCD-EFGH",
          verificationUri: "https://example.com/v",
        });
        return { provider: "kimi-coding", credentialType: "oauth", models: [] };
      },
    );

    const program = new Command("dimi").exitOverride();
    registerLoginCommand(program);

    await expect(program.parseAsync(["node", "dimi", "login"])).rejects.toThrow(ExitCalled);

    const writtenChunks = stderrSpy.mock.calls.map((call: unknown[]) => String(call[0]));
    expect(writtenChunks.some((chunk: string) => chunk.includes("ABCD-EFGH"))).toBe(true);
    expect(writtenChunks.some((chunk: string) => chunk.includes("https://example.com/v"))).toBe(
      true,
    );
    expect(openUrl).toHaveBeenCalledWith("https://example.com/v");
    expect(exitSpy).toHaveBeenCalledWith(0);
  });

  it("exits 1 when auth.login throws", async () => {
    mockLogin.mockRejectedValue(new Error("boom"));

    const program = new Command("dimi").exitOverride();
    registerLoginCommand(program);

    await expect(program.parseAsync(["node", "dimi", "login"])).rejects.toThrow(ExitCalled);

    const writtenChunks = stderrSpy.mock.calls.map((call: unknown[]) => String(call[0]));
    expect(writtenChunks.some((chunk: string) => chunk.includes("boom"))).toBe(true);
    expect(exitSpy).toHaveBeenCalledWith(1);
  });
});
