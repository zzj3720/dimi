import { describe, expect, it } from "vitest";

import type { TranscriptItem } from "@dimi-agent/transcript";

import { buildTimeline, summarizeTools } from "../src/timeline";

describe("mobile transcript presentation", () => {
  it("keeps user and assistant messages readable while collapsing consecutive tools", () => {
    const timeline = buildTimeline([
      turn("turn-1", "Inspect the project", [
        text("a1", "I will inspect it."),
        tool("read-1", "Read", "done"),
        tool("read-2", "Read", "done"),
        tool("search-1", "Search", "done"),
        text("a2", "The implementation is ready."),
      ]),
    ]);

    expect(timeline).toEqual([
      { id: "user:turn-1", kind: "user", text: "Inspect the project" },
      { id: "assistant:a1", kind: "assistant", text: "I will inspect it." },
      {
        id: "tools:read-1:read-2:search-1",
        kind: "tools",
        tools: [
          expect.objectContaining({ toolCallId: "read-1", name: "Read" }),
          expect.objectContaining({ toolCallId: "read-2", name: "Read" }),
          expect.objectContaining({ toolCallId: "search-1", name: "Search" }),
        ],
      },
      { id: "assistant:a2", kind: "assistant", text: "The implementation is ready." },
    ]);
    const group = timeline[2];
    expect(group?.kind).toBe("tools");
    if (group?.kind === "tools") {
      expect(summarizeTools(group.tools)).toBe("Used 3 tools · 2 Read · 1 Search");
    }
  });

  it("preserves notices and tool errors in stable transcript order", () => {
    const timeline = buildTimeline([
      turn("turn-2", "Run checks", [
        tool("bash-1", "Bash", "error"),
        {
          kind: "notice",
          frameId: "notice-1",
          message: "The command failed.",
        },
        text("a3", "I will fix the failure."),
      ]),
    ]);

    expect(timeline.map((entry) => entry.kind)).toEqual(["user", "tools", "notice", "assistant"]);
    const group = timeline[1];
    expect(group?.kind).toBe("tools");
    if (group?.kind === "tools") expect(group.tools[0]?.state).toBe("error");
  });

  it("does not create empty message rows for transcript frames the mobile client does not render", () => {
    const timeline = buildTimeline([
      turn("turn-3", "Continue", [
        {
          kind: "thinking",
          frameId: "think-1",
          text: "private reasoning",
        },
        text("a4", "Done."),
      ]),
    ]);

    expect(timeline).toEqual([
      { id: "user:turn-3", kind: "user", text: "Continue" },
      { id: "assistant:a4", kind: "assistant", text: "Done." },
    ]);
  });

  it("does not render internal task notifications as user messages", () => {
    const timeline = buildTimeline([
      turn(
        "turn-4",
        '<notification type="task.completed">Background tool completed.</notification>',
        [text("a5", "The background task is complete.")],
        { kind: "task", taskId: "task-1" },
      ),
    ]);

    expect(timeline).toEqual([
      {
        id: "assistant:a5",
        kind: "assistant",
        text: "The background task is complete.",
      },
    ]);
  });
});

function turn(
  turnId: string,
  prompt: string,
  frames: Extract<TranscriptItem, { kind: "turn" }>["steps"][number]["frames"],
  origin?: Extract<TranscriptItem, { kind: "turn" }>["origin"],
): TranscriptItem {
  return {
    kind: "turn",
    turnId,
    ordinal: 0,
    state: "completed",
    origin: origin ?? { kind: "user" },
    prompt,
    steps: [
      {
        kind: "step",
        stepId: `${turnId}.1`,
        turnId,
        ordinal: 1,
        state: "completed",
        frames,
      },
    ],
  };
}

function text(
  frameId: string,
  value: string,
): Extract<
  Extract<TranscriptItem, { kind: "turn" }>["steps"][number]["frames"][number],
  { kind: "text" }
> {
  return { kind: "text", frameId, role: "assistant", text: value };
}

function tool(
  toolCallId: string,
  name: string,
  state: "done" | "error",
): Extract<
  Extract<TranscriptItem, { kind: "turn" }>["steps"][number]["frames"][number],
  { kind: "tool" }
> {
  return {
    kind: "tool",
    frameId: `frame-${toolCallId}`,
    toolCallId,
    name,
    state,
  };
}
