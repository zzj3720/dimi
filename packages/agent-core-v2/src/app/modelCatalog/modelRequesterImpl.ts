import type {
  AssistantMessage,
  AssistantMessageEvent,
  Context,
  ImageContent,
  Message as RuntimeMessage,
  ModelThinkingLevel,
  TextContent,
  ThinkingContent,
  Tool as RuntimeTool,
  ToolCall as RuntimeToolCall,
  ToolResultMessage,
  Usage,
  UserMessage,
} from "#/app/providerRuntime/types";

import { AsyncEventQueue } from "#/_base/asyncEventQueue";
import { IProviderRuntime } from "#/app/providerRuntime/providerRuntime";
import { providerRuntimeError } from "#/app/providerRuntime/errors";
import type { ContentPart, Message, StreamedMessagePart, ToolCall } from "#/llmProtocol/message";
import type { FinishReason } from "#/llmProtocol/provider";
import type { TokenUsage } from "#/llmProtocol/usage";

import type { Model } from "./catalog";
import type {
  ModelRequestEvent,
  ModelRequestInput,
  ModelRequestParams,
  ModelRequester,
} from "./modelRequester";

export class ModelRequesterImpl implements ModelRequester {
  constructor(
    readonly model: Model,
    private readonly runtime: Pick<IProviderRuntime, "streamSimple">,
  ) {}

  request(
    input: ModelRequestInput,
    signal?: AbortSignal,
    params?: ModelRequestParams,
  ): AsyncIterable<ModelRequestEvent> {
    const queue = new AsyncEventQueue<ModelRequestEvent>();
    void this.run(input, signal, params, queue).then(
      () => {
        queue.end();
      },
      (error) => {
        queue.fail(error);
      },
    );
    return queue;
  }

  private async run(
    input: ModelRequestInput,
    signal: AbortSignal | undefined,
    params: ModelRequestParams | undefined,
    queue: AsyncEventQueue<ModelRequestEvent>,
  ): Promise<void> {
    signal?.throwIfAborted();
    const startedAt = Date.now();
    let firstPartAt: number | undefined;
    let emittedToolCalls = 0;

    const context: Context = {
      systemPrompt: input.systemPrompt,
      tools: input.tools.filter((tool) => tool.deferred !== true).map(toRuntimeTool),
      messages: input.messages
        .map((message) => toRuntimeMessage(message, this.model))
        .filter((message): message is RuntimeMessage => message !== undefined),
    };
    const stream = this.runtime.streamSimple(this.model, context, {
      signal,
      temperature: params?.sampling?.temperature,
      topP: params?.sampling?.topP,
      maxTokens: clampOutputTokens(params),
      cacheKey: params?.cacheKey,
      reasoning: toThinkingLevel(params?.thinkingEffort),
      thinkingKeep: params?.thinkingKeep,
      responseFormat: toResponseFormat(input.responseFormat),
      onResponse: (response) => {
        params?.onTraceId?.(response.headers["x-trace-id"] ?? null);
      },
    });

    for await (const event of stream) {
      firstPartAt ??= isContentEvent(event) ? Date.now() : undefined;
      const part = toDimiPart(event);
      if (part !== undefined) {
        if (part.type === "function") emittedToolCalls += 1;
        queue.push({ type: "part", part });
      }
      if (event.type === "done") {
        queue.push({
          type: "usage",
          usage: toTokenUsage(event.message.usage),
          model: event.message.model,
        });
        queue.push({
          type: "finish",
          message: toDimiAssistantMessage(event.message),
          providerFinishReason: toFinishReason(event.message.finishReason ?? event.reason),
          rawFinishReason: event.message.rawStopReason ?? event.reason,
          id: event.message.responseId,
          traceId: event.message.traceId,
        });
      }
      if (event.type === "error") {
        if (event.reason === "aborted") {
          throw new DOMException(
            event.error.errorMessage ?? "Provider request aborted",
            "AbortError",
          );
        }
        throw providerRuntimeError(
          event.error.errorMessage ?? `Provider ${event.reason}`,
          event.error,
          event.cause,
        );
      }
    }

    if (firstPartAt !== undefined) {
      queue.push({
        type: "timing",
        firstTokenLatencyMs: Math.max(0, firstPartAt - startedAt),
        streamDurationMs: Math.max(0, Date.now() - firstPartAt),
        clientConsumeMs: Math.max(0, Date.now() - firstPartAt),
      });
    }
    void emittedToolCalls;
  }
}

function clampOutputTokens(params: ModelRequestParams | undefined): number | undefined {
  const requested = params?.maxCompletionTokens;
  if (requested === undefined) return undefined;
  const context = params?.maxContextTokens;
  const used = params?.usedContextTokens;
  if (context === undefined || used === undefined) return requested;
  return Math.max(1, Math.min(requested, context - used));
}

function toResponseFormat(
  responseFormat: ModelRequestInput["responseFormat"],
): import("#/app/providerRuntime/types").ModelsSimpleStreamOptions["responseFormat"] {
  if (responseFormat === undefined) return undefined;
  if (responseFormat.type === "json_object") return { type: "json_object" };
  if (responseFormat.type === "json_schema") {
    return {
      type: "json_schema",
      jsonSchema: {
        name: responseFormat.jsonSchema.name,
        schema: responseFormat.jsonSchema.schema,
        strict: responseFormat.jsonSchema.strict,
      },
    };
  }
  return { type: "text" };
}

function toRuntimeTool(tool: ModelRequestInput["tools"][number]): RuntimeTool {
  return {
    name: tool.name,
    description: tool.description,
    parameters: tool.parameters,
  };
}

function toRuntimeMessage(message: Message, model: Model): RuntimeMessage | undefined {
  const timestamp = Date.now();
  switch (message.role) {
    case "system":
      return undefined;
    case "user":
      return {
        role: "user",
        content: toRuntimeUserContent(message.content),
        timestamp,
      } satisfies UserMessage;
    case "assistant":
      return {
        role: "assistant",
        content: [
          ...message.content.flatMap(toRuntimeAssistantContent),
          ...message.toolCalls.map(toRuntimeToolCall),
        ],
        api: model.api,
        provider: model.provider,
        model: model.id,
        usage: emptyRuntimeUsage(),
        stopReason: message.toolCalls.length > 0 ? "toolUse" : "stop",
        timestamp,
      } satisfies AssistantMessage;
    case "tool":
      return {
        role: "toolResult",
        toolCallId: message.toolCallId ?? "",
        toolName: message.name ?? "tool",
        content: message.content.flatMap(toRuntimeResultContent),
        isError: false,
        timestamp,
      } satisfies ToolResultMessage;
  }
}

function toRuntimeUserContent(content: readonly ContentPart[]): UserMessage["content"] {
  const parts = content.flatMap(toRuntimeResultContent);
  if (parts.length === 1 && parts[0]?.type === "text") return parts[0].text;
  return parts;
}

function toRuntimeResultContent(part: ContentPart): (TextContent | ImageContent)[] {
  switch (part.type) {
    case "text":
      return [{ type: "text", text: part.text }];
    case "think":
      return [{ type: "text", text: part.think }];
    case "image_url": {
      const image = parseDataUrl(part.imageUrl.url);
      return [image ?? { type: "image", url: part.imageUrl.url }];
    }
    case "audio_url":
      return [{ type: "text", text: `[audio: ${part.audioUrl.url}]` }];
    case "video_url":
      return [{ type: "text", text: `[video: ${part.videoUrl.url}]` }];
  }
}

function toRuntimeAssistantContent(part: ContentPart): (TextContent | ThinkingContent)[] {
  if (part.type === "think") {
    return [
      {
        type: "thinking",
        thinking: part.think,
        thinkingSignature: part.encrypted,
      },
    ];
  }
  if (part.type === "text") return [{ type: "text", text: part.text }];
  if (part.type === "image_url") {
    return [{ type: "text", text: `[image: ${part.imageUrl.url}]` }];
  }
  if (part.type === "audio_url") {
    return [{ type: "text", text: `[audio: ${part.audioUrl.url}]` }];
  }
  return [{ type: "text", text: `[video: ${part.videoUrl.url}]` }];
}

function toRuntimeToolCall(call: ToolCall): RuntimeToolCall {
  return {
    type: "toolCall",
    id: call.id,
    name: call.name,
    arguments: parseArguments(call.arguments),
    argumentsRaw: call.arguments ?? undefined,
    thoughtSignature:
      typeof call.extras?.["thoughtSignature"] === "string"
        ? call.extras["thoughtSignature"]
        : undefined,
  };
}

function parseArguments(value: string | null): Record<string, unknown> {
  if (value === null || value.trim().length === 0) return {};
  try {
    const parsed: unknown = JSON.parse(value);
    return typeof parsed === "object" && parsed !== null && !Array.isArray(parsed)
      ? (parsed as Record<string, unknown>)
      : {};
  } catch {
    return {};
  }
}

function parseDataUrl(url: string): ImageContent | undefined {
  const match = /^data:([^;,]+);base64,(.*)$/su.exec(url);
  if (match === null) return undefined;
  return { type: "image", mimeType: match[1]!, data: match[2]! };
}

function toDimiPart(event: AssistantMessageEvent): StreamedMessagePart | undefined {
  switch (event.type) {
    case "text_delta":
      return { type: "text", text: event.delta };
    case "thinking_delta":
      return { type: "think", think: event.delta };
    case "toolcall_start":
      return {
        type: "function",
        id: event.id,
        name: event.name,
        arguments: null,
        _streamIndex: event.index,
      };
    case "toolcall_delta":
      return {
        type: "tool_call_part",
        argumentsPart: event.delta,
        index: event.index,
      };
    case "toolcall_end":
    case "start":
    case "done":
    case "error":
      return undefined;
  }
}

function isContentEvent(event: AssistantMessageEvent): boolean {
  return (
    event.type.endsWith("_delta") ||
    event.type === "toolcall_start" ||
    event.type === "toolcall_end"
  );
}

function toDimiAssistantMessage(message: AssistantMessage): Message {
  return {
    role: "assistant",
    content: message.content.flatMap((part): ContentPart[] => {
      switch (part.type) {
        case "text":
          return [{ type: "text", text: part.text }];
        case "thinking":
          return [
            {
              type: "think",
              think: part.thinking,
              encrypted: part.thinkingSignature,
            },
          ];
        case "toolCall":
          return [];
      }
      return unreachable(part);
    }),
    toolCalls: message.content.flatMap((part): ToolCall[] =>
      part.type === "toolCall"
        ? [
            {
              type: "function",
              id: part.id,
              name: part.name,
              arguments: part.argumentsRaw ?? JSON.stringify(part.arguments),
              extras:
                part.thoughtSignature === undefined
                  ? undefined
                  : { thoughtSignature: part.thoughtSignature },
            },
          ]
        : [],
    ),
  };
}

function toTokenUsage(usage: Usage): TokenUsage {
  return {
    inputOther: usage.input,
    output: usage.output,
    inputCacheRead: usage.cacheRead,
    inputCacheCreation: usage.cacheWrite,
  };
}

function emptyRuntimeUsage(): Usage {
  return {
    input: 0,
    output: 0,
    cacheRead: 0,
    cacheWrite: 0,
    totalTokens: 0,
    cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, total: 0 },
  };
}

function toThinkingLevel(effort: string | undefined): ModelThinkingLevel | undefined {
  if (effort === undefined) return undefined;
  if (effort === "off") return "off";
  if (effort === "on") return "medium";
  if (
    effort === "minimal" ||
    effort === "low" ||
    effort === "medium" ||
    effort === "high" ||
    effort === "xhigh" ||
    effort === "max"
  ) {
    return effort;
  }
  return "medium";
}

function toFinishReason(
  reason: AssistantMessage["finishReason"] | "stop" | "length" | "toolUse",
): FinishReason {
  switch (reason) {
    case "stop":
      return "completed";
    case "length":
      return "truncated";
    case "toolUse":
      return "tool_calls";
    case undefined:
      return "other";
    case "completed":
    case "filtered":
    case "other":
    case "paused":
    case "tool_calls":
    case "truncated":
      return reason;
  }
}

function unreachable(value: never): never {
  throw new TypeError(`Unsupported provider content: ${JSON.stringify(value)}`);
}
