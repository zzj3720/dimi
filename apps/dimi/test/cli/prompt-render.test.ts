import { describe, expect, it } from "vitest";

import {
  PromptTranscriptWriter,
  type PromptOutput,
} from "../../src/cli/prompt-render";

function captureOutput(): { output: PromptOutput; text: () => string } {
  let text = "";
  return {
    output: { write: (chunk: string) => ((text += chunk), true) },
    text: () => text,
  };
}

describe("PromptTranscriptWriter (text -p tool transcript)", () => {
  it("records the tool call name and args on stderr", () => {
    const stdout = captureOutput();
    const stderr = captureOutput();
    const writer = new PromptTranscriptWriter(stdout.output, stderr.output);

    writer.writeToolCall("call_1", "Bash", { command: "git status" });
    writer.finish();

    expect(stdout.text()).toBe("");
    expect(stderr.text()).toContain("⚒");
    expect(stderr.text()).toContain("Bash({");
    expect(stderr.text()).toContain('"command":"git status"');
  });

  it("records the tool result on stderr", () => {
    const stdout = captureOutput();
    const stderr = captureOutput();
    const writer = new PromptTranscriptWriter(stdout.output, stderr.output);

    writer.writeToolCall("call_1", "Bash", { command: "git status" });
    writer.writeToolResult("call_1", "On branch master\nnothing to commit");
    writer.finish();

    expect(stdout.text()).toBe("");
    expect(stderr.text()).toContain("⚒ result");
    expect(stderr.text()).toContain("On branch master");
  });

  it("truncates oversized tool args and results", () => {
    const stdout = captureOutput();
    const stderr = captureOutput();
    const writer = new PromptTranscriptWriter(stdout.output, stderr.output);

    const huge = "x".repeat(10_000);
    writer.writeToolCall("call_1", "Bash", { command: huge });
    writer.writeToolResult("call_1", huge);
    writer.finish();

    const text = stderr.text();
    expect(text).toContain("…");
    expect(text.length).toBeLessThan(5_000);
  });

  it("announces transient provider retries", () => {
    const stdout = captureOutput();
    const stderr = captureOutput();
    const writer = new PromptTranscriptWriter(stdout.output, stderr.output);

    writer.writeRetrying({
      failedAttempt: 1,
      nextAttempt: 2,
      maxAttempts: 10,
      delayMs: 500,
      errorName: "CONNECTION_ERROR",
      errorMessage: "connect ECONNREFUSED",
    });
    writer.finish();

    expect(stdout.text()).toBe("");
    expect(stderr.text()).toContain("↻ retry 1/10");
    expect(stderr.text()).toContain("CONNECTION_ERROR");
    expect(stderr.text()).toContain("500ms");
  });

  it("keeps assistant text on stdout only", () => {
    const stdout = captureOutput();
    const stderr = captureOutput();
    const writer = new PromptTranscriptWriter(stdout.output, stderr.output);

    writer.writeToolCall("call_1", "Bash", { command: "ls" });
    writer.writeAssistantDelta("done");
    writer.finish();

    expect(stdout.text()).toContain("done");
    expect(stdout.text()).not.toContain("Bash");
  });
});
