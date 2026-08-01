import type { ToolCallFrame, TranscriptItem } from "@dimi-agent/transcript";

export type TimelineEntry =
  | { readonly id: string; readonly kind: "user" | "assistant" | "notice"; readonly text: string }
  | { readonly id: string; readonly kind: "tools"; readonly tools: readonly ToolCallFrame[] };

export function buildTimeline(items: readonly TranscriptItem[]): readonly TimelineEntry[] {
  const timeline: TimelineEntry[] = [];
  let pendingTools: ToolCallFrame[] = [];

  const flushTools = (): void => {
    if (pendingTools.length === 0) return;
    timeline.push({
      id: `tools:${pendingTools.map((tool) => tool.toolCallId).join(":")}`,
      kind: "tools",
      tools: pendingTools,
    });
    pendingTools = [];
  };

  for (const item of items) {
    if (item.kind !== "turn") continue;
    if (item.origin.kind === "user" && item.prompt !== undefined && item.prompt.length > 0) {
      flushTools();
      timeline.push({ id: `user:${item.turnId}`, kind: "user", text: item.prompt });
    }
    for (const step of item.steps) {
      for (const frame of step.frames) {
        if (frame.kind === "tool") {
          pendingTools.push(frame);
          continue;
        }
        if (frame.kind === "text" && frame.role === "assistant" && frame.text.length > 0) {
          flushTools();
          timeline.push({
            id: `assistant:${frame.frameId}`,
            kind: "assistant",
            text: frame.text,
          });
          continue;
        }
        if (frame.kind === "notice") {
          flushTools();
          timeline.push({ id: `notice:${frame.frameId}`, kind: "notice", text: frame.message });
        }
      }
    }
  }
  flushTools();
  return timeline;
}

export function summarizeTools(tools: readonly ToolCallFrame[]): string {
  const counts = new Map<string, number>();
  for (const tool of tools) counts.set(tool.name, (counts.get(tool.name) ?? 0) + 1);
  const details = [...counts]
    .slice(0, 2)
    .map(([name, count]) => `${count} ${name}`)
    .join(" · ");
  return `Used ${tools.length} tool${tools.length === 1 ? "" : "s"}${details.length > 0 ? ` · ${details}` : ""}`;
}
