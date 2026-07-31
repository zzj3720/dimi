import type {
  AssistantMessage,
  AssistantMessageEvent,
  AuthResult,
  Context,
  Model,
  ModelsSimpleStreamOptions,
  ProviderHeaders,
  ToolCall,
  Usage,
} from "./types";

export function streamProvider(
  model: Model,
  context: Context,
  auth: AuthResult,
  options?: ModelsSimpleStreamOptions,
): AsyncIterable<AssistantMessageEvent> {
  switch (model.api) {
    case "anthropic-messages":
      return streamAnthropic(model, context, auth, options);
    case "openai-responses":
    case "openai-codex-responses":
      return streamOpenAIResponses(model, context, auth, options);
    case "openai-completions":
      return streamOpenAIChat(model, context, auth, options);
    case "google-generative-ai":
      return failedStream(model, "Google Generative AI streaming is not configured");
    default:
      return failedStream(model, `Unsupported provider API: ${model.api}`);
  }
}

async function* streamOpenAIChat(
  model: Model,
  context: Context,
  auth: AuthResult,
  options?: ModelsSimpleStreamOptions,
): AsyncIterable<AssistantMessageEvent> {
  const output = emptyAssistant(model);
  yield { type: "start", partial: output };
  try {
    const response = await fetch(
      `${trimSlash(auth.auth.baseUrl ?? model.baseUrl)}/chat/completions`,
      {
        method: "POST",
        headers: requestHeaders(model.headers, auth, {
          accept: "text/event-stream",
          "content-type": "application/json",
        }),
        body: JSON.stringify({
          model: model.id,
          stream: true,
          stream_options: { include_usage: true },
          messages: toOpenAIChatMessages(context),
          tools:
            context.tools?.map((tool) => ({
              type: "function",
              function: {
                name: tool.name,
                description: tool.description,
                parameters: tool.parameters,
              },
            })) ?? undefined,
          temperature: options?.temperature,
          max_tokens: options?.maxTokens,
          reasoning_effort: options?.reasoning,
        }),
        signal: options?.signal,
      },
    );
    const responseHeaders = headersRecord(response.headers);
    output.traceId = responseHeaders["x-trace-id"];
    options?.onResponse?.({ headers: responseHeaders });
    await assertOk(response);

    const calls = new Map<
      number,
      { id: string; name: string; arguments: string; started: boolean }
    >();
    let finishReason: string | undefined;
    for await (const data of sseJson(response)) {
      const choice = firstRecord(data["choices"]);
      const delta = record(choice?.["delta"]);
      const text = string(delta?.["content"]);
      if (text !== undefined) {
        appendText(output, text);
        yield { type: "text_delta", delta: text, partial: output };
      }
      const thinking =
        string(delta?.["reasoning_content"]) ??
        string(delta?.["reasoning"]) ??
        string(delta?.["thinking"]);
      if (thinking !== undefined) {
        appendThinking(output, thinking);
        yield { type: "thinking_delta", delta: thinking, partial: output };
      }
      for (const raw of array(delta?.["tool_calls"])) {
        const item = record(raw);
        const index = number(item?.["index"]) ?? calls.size;
        const fn = record(item?.["function"]);
        const current = calls.get(index) ?? {
          id: "",
          name: "",
          arguments: "",
          started: false,
        };
        current.id += string(item?.["id"]) ?? "";
        current.name += string(fn?.["name"]) ?? "";
        const argumentsDelta = string(fn?.["arguments"]) ?? "";
        current.arguments += argumentsDelta;
        calls.set(index, current);
        if (!current.started && (current.id !== "" || current.name !== "")) {
          current.started = true;
          yield {
            type: "toolcall_start",
            index,
            id: current.id,
            name: current.name,
            partial: output,
          };
        }
        if (argumentsDelta !== "") {
          yield {
            type: "toolcall_delta",
            index,
            delta: argumentsDelta,
            partial: output,
          };
        }
      }
      finishReason = string(choice?.["finish_reason"]) ?? finishReason;
      const rawUsage = record(data["usage"]);
      if (rawUsage !== undefined) output.usage = openAIUsage(rawUsage);
      output.responseId = string(data["id"]) ?? output.responseId;
      output.responseModel = string(data["model"]) ?? output.responseModel;
    }
    for (const call of calls.values()) {
      const toolCall = parsedToolCall(call);
      output.content.push(toolCall);
      yield { type: "toolcall_end", toolCall, partial: output };
    }
    output.stopReason =
      calls.size > 0 || finishReason === "tool_calls"
        ? "toolUse"
        : finishReason === "length"
          ? "length"
          : "stop";
    output.finishReason = normalizedFinishReason(output.stopReason, finishReason);
    output.rawStopReason = finishReason;
    yield {
      type: "done",
      reason: output.stopReason,
      message: output,
    };
  } catch (error) {
    yield errorEvent(model, error, options?.signal, output);
  }
}

async function* streamOpenAIResponses(
  model: Model,
  context: Context,
  auth: AuthResult,
  options?: ModelsSimpleStreamOptions,
): AsyncIterable<AssistantMessageEvent> {
  const output = emptyAssistant(model);
  yield { type: "start", partial: output };
  try {
    const codex = model.api === "openai-codex-responses";
    const endpoint = codex
      ? `${trimSlash(auth.auth.baseUrl ?? model.baseUrl)}/codex/responses`
      : `${trimSlash(auth.auth.baseUrl ?? model.baseUrl)}/responses`;
    const token = auth.auth.apiKey;
    const accountId = token === undefined ? undefined : accountIdFromJwt(token);
    const sessionId = options?.sessionId?.slice(0, 64);
    const headers = requestHeaders(model.headers, auth, {
      accept: "text/event-stream",
      "content-type": "application/json",
      ...(codex
        ? {
            originator: "kimi-code",
            "openai-beta": "responses=experimental",
            ...(accountId === undefined ? {} : { "chatgpt-account-id": accountId }),
          }
        : {}),
      ...(sessionId === undefined
        ? {}
        : {
            "session-id": sessionId,
            "x-client-request-id": sessionId,
          }),
    });
    const response = await fetch(endpoint, {
      method: "POST",
      headers,
      body: JSON.stringify({
        model: model.id,
        stream: true,
        store: false,
        instructions: context.systemPrompt,
        input: toResponsesInput(context),
        tools:
          context.tools?.map((tool) => ({
            type: "function",
            name: tool.name,
            description: tool.description,
            parameters: tool.parameters,
          })) ?? undefined,
        parallel_tool_calls: true,
        temperature: options?.temperature,
        max_output_tokens: options?.maxTokens,
        reasoning:
          options?.reasoning === undefined
            ? undefined
            : { effort: options.reasoning, summary: "auto" },
      }),
      signal: options?.signal,
    });
    const responseHeaders = headersRecord(response.headers);
    output.traceId = responseHeaders["x-trace-id"];
    options?.onResponse?.({ headers: responseHeaders });
    await assertOk(response);

    const calls = new Map<string, { id: string; name: string; arguments: string }>();
    let terminal = false;
    for await (const data of sseJson(response)) {
      const type = string(data["type"]);
      if (type === "response.output_text.delta") {
        const delta = string(data["delta"]) ?? "";
        appendText(output, delta);
        yield { type: "text_delta", delta, partial: output };
      } else if (
        type === "response.reasoning_summary_text.delta" ||
        type === "response.reasoning_text.delta"
      ) {
        const delta = string(data["delta"]) ?? "";
        appendThinking(output, delta);
        yield { type: "thinking_delta", delta, partial: output };
      } else if (type === "response.output_item.added") {
        const item = record(data["item"]);
        if (item?.["type"] === "function_call") {
          const key = string(item["id"]) ?? string(item["call_id"]) ?? String(calls.size);
          calls.set(key, {
            id: string(item["call_id"]) ?? key,
            name: string(item["name"]) ?? "",
            arguments: string(item["arguments"]) ?? "",
          });
          const call = calls.get(key)!;
          yield {
            type: "toolcall_start",
            index: key,
            id: call.id,
            name: call.name,
            partial: output,
          };
        }
      } else if (type === "response.function_call_arguments.delta") {
        const key = string(data["item_id"]) ?? string(data["call_id"]) ?? "";
        const call = calls.get(key);
        const delta = string(data["delta"]) ?? "";
        if (call !== undefined) call.arguments += delta;
        if (delta !== "") {
          yield { type: "toolcall_delta", index: key, delta, partial: output };
        }
      } else if (type === "response.function_call_arguments.done") {
        const key = string(data["item_id"]) ?? string(data["call_id"]) ?? "";
        const call = calls.get(key);
        if (call !== undefined) call.arguments = string(data["arguments"]) ?? call.arguments;
      } else if (type === "response.output_item.done") {
        const item = record(data["item"]);
        if (item?.["type"] === "function_call") {
          const key = string(item["id"]) ?? string(item["call_id"]) ?? String(calls.size);
          calls.set(key, {
            id: string(item["call_id"]) ?? key,
            name: string(item["name"]) ?? "",
            arguments: string(item["arguments"]) ?? calls.get(key)?.arguments ?? "",
          });
        }
      } else if (
        type === "response.completed" ||
        type === "response.incomplete" ||
        type === "response.failed"
      ) {
        const raw = record(data["response"]);
        output.responseId = string(raw?.["id"]);
        output.responseModel = string(raw?.["model"]);
        const usage = record(raw?.["usage"]);
        if (usage !== undefined) output.usage = responsesUsage(usage);
        output.rawStopReason =
          string(raw?.["status"]) ??
          (type === "response.incomplete"
            ? "incomplete"
            : type === "response.failed"
              ? "failed"
              : "completed");
        if (type === "response.failed") {
          throw new Error(providerErrorMessage(raw?.["error"] ?? data["error"] ?? raw ?? data));
        }
        terminal = true;
        break;
      } else if (type === "error") {
        throw new Error(providerErrorMessage(data["error"] ?? data));
      }
    }
    if (!terminal)
      throw new Error("OpenAI Responses stream ended before a terminal response event");
    for (const call of calls.values()) {
      const toolCall = parsedToolCall(call);
      output.content.push(toolCall);
      yield { type: "toolcall_end", toolCall, partial: output };
    }
    output.stopReason =
      output.rawStopReason === "incomplete" ? "length" : calls.size > 0 ? "toolUse" : "stop";
    output.finishReason = normalizedFinishReason(output.stopReason, output.rawStopReason);
    yield { type: "done", reason: output.stopReason, message: output };
  } catch (error) {
    yield errorEvent(model, error, options?.signal, output);
  }
}

async function* streamAnthropic(
  model: Model,
  context: Context,
  auth: AuthResult,
  options?: ModelsSimpleStreamOptions,
): AsyncIterable<AssistantMessageEvent> {
  const output = emptyAssistant(model);
  yield { type: "start", partial: output };
  try {
    const response = await fetch(`${trimSlash(auth.auth.baseUrl ?? model.baseUrl)}/messages`, {
      method: "POST",
      headers: requestHeaders(model.headers, auth, {
        accept: "text/event-stream",
        "content-type": "application/json",
        "anthropic-version": "2023-06-01",
        ...(auth.auth.apiKey === undefined ? {} : { "x-api-key": auth.auth.apiKey }),
      }),
      body: JSON.stringify({
        model: model.id,
        stream: true,
        system: context.systemPrompt,
        messages: toAnthropicMessages(context),
        tools: context.tools?.map((tool) => ({
          name: tool.name,
          description: tool.description,
          input_schema: tool.parameters,
        })),
        temperature: options?.temperature,
        max_tokens: options?.maxTokens ?? model.maxTokens,
        thinking:
          options?.reasoning === undefined
            ? undefined
            : {
                type: "enabled",
                budget_tokens: Math.min(
                  thinkingBudget(options.reasoning),
                  Math.max(1_024, (options.maxTokens ?? model.maxTokens) - 1),
                ),
              },
      }),
      signal: options?.signal,
    });
    const responseHeaders = headersRecord(response.headers);
    output.traceId = responseHeaders["x-trace-id"];
    options?.onResponse?.({ headers: responseHeaders });
    await assertOk(response);
    const calls = new Map<number, { id: string; name: string; arguments: string }>();
    const thinkingBlocks = new Map<
      number,
      Extract<AssistantMessage["content"][number], { type: "thinking" }>
    >();
    let stopReason: string | undefined;
    let lastStopDetails: Record<string, unknown> | undefined;
    for await (const data of sseJson(response)) {
      const type = string(data["type"]);
      if (type === "content_block_start") {
        const index = number(data["index"]) ?? calls.size;
        const block = record(data["content_block"]);
        if (block?.["type"] === "tool_use") {
          calls.set(index, {
            id: string(block["id"]) ?? "",
            name: string(block["name"]) ?? "",
            arguments: JSON.stringify(block["input"] ?? {}).replace(/^\{\}$/u, ""),
          });
          const call = calls.get(index)!;
          yield {
            type: "toolcall_start",
            index,
            id: call.id,
            name: call.name,
            partial: output,
          };
        } else if (block?.["type"] === "thinking") {
          const thinking = { type: "thinking" as const, thinking: "" };
          output.content.push(thinking);
          thinkingBlocks.set(index, thinking);
        }
      } else if (type === "content_block_delta") {
        const index = number(data["index"]) ?? 0;
        const delta = record(data["delta"]);
        if (delta?.["type"] === "text_delta") {
          const text = string(delta["text"]) ?? "";
          appendText(output, text);
          yield { type: "text_delta", delta: text, partial: output };
        } else if (delta?.["type"] === "thinking_delta") {
          const thinking = string(delta["thinking"]) ?? "";
          const block = thinkingBlocks.get(index);
          if (block === undefined) {
            const created = { type: "thinking" as const, thinking: "" };
            output.content.push(created);
            thinkingBlocks.set(index, created);
            created.thinking += thinking;
          } else {
            block.thinking += thinking;
          }
          yield { type: "thinking_delta", delta: thinking, partial: output };
        } else if (delta?.["type"] === "signature_delta") {
          const block = thinkingBlocks.get(index);
          if (block !== undefined)
            block.thinkingSignature = `${block.thinkingSignature ?? ""}${string(delta["signature"]) ?? ""}`;
        } else if (delta?.["type"] === "input_json_delta") {
          const call = calls.get(index);
          const argumentsDelta = string(delta["partial_json"]) ?? "";
          if (call !== undefined) call.arguments += argumentsDelta;
          if (argumentsDelta !== "") {
            yield {
              type: "toolcall_delta",
              index,
              delta: argumentsDelta,
              partial: output,
            };
          }
        }
      } else if (type === "message_delta") {
        const delta = record(data["delta"]);
        stopReason = string(delta?.["stop_reason"]) ?? stopReason;
        lastStopDetails = record(delta?.["stop_details"]) ?? lastStopDetails;
        output.rawStopReason = stopReason;
        const usage = record(data["usage"]);
        if (usage !== undefined) {
          output.usage.output = number(usage["output_tokens"]) ?? output.usage.output;
        }
      } else if (type === "message_start") {
        const message = record(data["message"]);
        output.responseId = string(message?.["id"]);
        const usage = record(message?.["usage"]);
        if (usage !== undefined) output.usage = anthropicUsage(usage);
      } else if (type === "error") {
        throw new Error(providerErrorMessage(data["error"] ?? data));
      }
    }
    for (const call of calls.values()) {
      const toolCall = parsedToolCall(call);
      output.content.push(toolCall);
      yield { type: "toolcall_end", toolCall, partial: output };
    }
    output.usage.totalTokens =
      output.usage.input + output.usage.output + output.usage.cacheRead + output.usage.cacheWrite;
    const terminal = anthropicStopReason(stopReason, calls.size > 0, record(lastStopDetails));
    if (terminal.errorMessage !== undefined) throw new Error(terminal.errorMessage);
    output.stopReason = terminal.stopReason;
    output.finishReason = normalizedFinishReason(output.stopReason, stopReason);
    yield { type: "done", reason: output.stopReason, message: output };
  } catch (error) {
    yield errorEvent(model, error, options?.signal, output);
  }
}

function normalizedFinishReason(
  stopReason: "stop" | "length" | "toolUse",
  raw: string | undefined,
): NonNullable<AssistantMessage["finishReason"]> {
  if (raw === "filtered" || raw === "content_filter") return "filtered";
  if (raw === "paused") return "paused";
  if (stopReason === "toolUse") return "tool_calls";
  if (stopReason === "length") return "truncated";
  return "completed";
}

function anthropicStopReason(
  raw: string | undefined,
  hasTools: boolean,
  details?: Record<string, unknown>,
): { stopReason: "stop" | "length" | "toolUse"; errorMessage?: string } {
  if (hasTools || raw === "tool_use") return { stopReason: "toolUse" };
  if (raw === undefined || raw === "end_turn" || raw === "pause_turn" || raw === "stop_sequence") {
    return { stopReason: "stop" };
  }
  if (raw === "max_tokens") return { stopReason: "length" };
  if (raw === "refusal") {
    return {
      stopReason: "stop",
      errorMessage: string(details?.["explanation"]) ?? "The model refused to complete the request",
    };
  }
  if (raw === "sensitive")
    return { stopReason: "stop", errorMessage: "Provider stopped with: sensitive" };
  return { stopReason: "stop", errorMessage: `Unhandled Anthropic stop reason: ${raw}` };
}

async function* failedStream(model: Model, message: string): AsyncIterable<AssistantMessageEvent> {
  yield errorEvent(model, new Error(message));
}

function toOpenAIChatMessages(context: Context): unknown[] {
  const result: unknown[] = [];
  if (context.systemPrompt) result.push({ role: "system", content: context.systemPrompt });
  for (const message of context.messages) {
    if (message.role === "user") {
      result.push({
        role: "user",
        content:
          typeof message.content === "string"
            ? message.content
            : message.content.map((part) =>
                part.type === "text"
                  ? { type: "text", text: part.text }
                  : {
                      type: "image_url",
                      image_url: { url: imageUrl(part) },
                    },
              ),
      });
    } else if (message.role === "assistant") {
      const text = message.content
        .filter((part) => part.type === "text")
        .map((part) => part.text)
        .join("");
      const calls = message.content
        .filter((part): part is ToolCall => part.type === "toolCall")
        .map((call) => ({
          id: call.id,
          type: "function",
          function: { name: call.name, arguments: JSON.stringify(call.arguments) },
        }));
      result.push({
        role: "assistant",
        content: text || null,
        tool_calls: calls.length === 0 ? undefined : calls,
      });
    } else {
      result.push({
        role: "tool",
        tool_call_id: message.toolCallId,
        content: message.content
          .map((part) => (part.type === "text" ? part.text : "[image]"))
          .join("\n"),
      });
    }
  }
  return result;
}

function imageUrl(image: {
  readonly mimeType?: string;
  readonly data?: string;
  readonly url?: string;
}): string {
  if (image.url !== undefined) return image.url;
  return `data:${image.mimeType ?? "application/octet-stream"};base64,${image.data ?? ""}`;
}

function toResponsesInput(context: Context): unknown[] {
  const result: unknown[] = [];
  for (const message of context.messages) {
    if (message.role === "user") {
      result.push({
        role: "user",
        content:
          typeof message.content === "string"
            ? [{ type: "input_text", text: message.content }]
            : message.content.map((part) =>
                part.type === "text"
                  ? { type: "input_text", text: part.text }
                  : {
                      type: "input_image",
                      image_url: imageUrl(part),
                    },
              ),
      });
      continue;
    }
    if (message.role === "toolResult") {
      result.push({
        type: "function_call_output",
        call_id: message.toolCallId,
        output: message.content
          .map((part) => (part.type === "text" ? part.text : "[image]"))
          .join("\n"),
      });
      continue;
    }
    const text = message.content
      .filter((part) => part.type === "text")
      .map((part) => part.text)
      .join("");
    if (text.length > 0) {
      result.push({
        role: "assistant",
        content: [{ type: "output_text", text }],
      });
    }
    for (const part of message.content) {
      if (part.type === "toolCall") {
        result.push({
          type: "function_call",
          call_id: part.id,
          name: part.name,
          arguments: JSON.stringify(part.arguments),
        });
      }
    }
  }
  return result;
}

function toAnthropicMessages(context: Context): unknown[] {
  const result: unknown[] = [];
  for (const message of context.messages) {
    if (message.role === "user") {
      result.push({
        role: "user",
        content:
          typeof message.content === "string"
            ? message.content
            : message.content.map((part) =>
                part.type === "text"
                  ? { type: "text", text: part.text }
                  : {
                      type: "image",
                      source:
                        part.url === undefined
                          ? {
                              type: "base64",
                              media_type: part.mimeType ?? "application/octet-stream",
                              data: part.data ?? "",
                            }
                          : { type: "url", url: part.url },
                    },
              ),
      });
    } else if (message.role === "assistant") {
      result.push({
        role: "assistant",
        content: message.content.map((part) => {
          if (part.type === "text") return { type: "text", text: part.text };
          if (part.type === "thinking") {
            return { type: "thinking", thinking: part.thinking, signature: part.thinkingSignature };
          }
          return {
            type: "tool_use",
            id: part.id,
            name: part.name,
            input: part.arguments,
          };
        }),
      });
    } else {
      result.push({
        role: "user",
        content: [
          {
            type: "tool_result",
            tool_use_id: message.toolCallId,
            is_error: message.isError,
            content: message.content
              .map((part) => (part.type === "text" ? part.text : "[image]"))
              .join("\n"),
          },
        ],
      });
    }
  }
  return result;
}

async function* sseJson(response: Response): AsyncIterable<Record<string, unknown>> {
  if (response.body === null) throw new Error("Provider response has no body");
  const reader = response.body.getReader();
  const decoder = new TextDecoder();
  let buffer = "";
  while (true) {
    const { done, value } = await reader.read();
    buffer += decoder.decode(value, { stream: !done });
    const frames = buffer.split(/\r?\n\r?\n/u);
    buffer = frames.pop() ?? "";
    for (const frame of frames) {
      const data = frame
        .split(/\r?\n/u)
        .filter((line) => line.startsWith("data:"))
        .map((line) => line.slice(5).trimStart())
        .join("\n");
      if (data.length === 0 || data === "[DONE]") continue;
      const parsed: unknown = JSON.parse(data);
      if (record(parsed) !== undefined) yield parsed as Record<string, unknown>;
    }
    if (done) break;
  }
  const finalData = sseData(buffer);
  if (finalData !== undefined) yield finalData;
}

function sseData(frame: string): Record<string, unknown> | undefined {
  const data = frame
    .split(/\r?\n/u)
    .filter((line) => line.startsWith("data:"))
    .map((line) => line.slice(5).trimStart())
    .join("\n");
  if (data.length === 0 || data === "[DONE]") return undefined;
  const parsed: unknown = JSON.parse(data);
  return record(parsed);
}

function requestHeaders(
  modelHeaders: ProviderHeaders | undefined,
  auth: AuthResult,
  base: Record<string, string>,
): Headers {
  const headers = new Headers(base);
  for (const source of [modelHeaders, auth.auth.headers]) {
    for (const [name, value] of Object.entries(source ?? {})) {
      if (value === null) headers.delete(name);
      else headers.set(name, value);
    }
  }
  if (
    auth.auth.apiKey !== undefined &&
    !headers.has("authorization") &&
    !headers.has("x-api-key")
  ) {
    headers.set("authorization", `Bearer ${auth.auth.apiKey}`);
  }
  return headers;
}

async function assertOk(response: Response): Promise<void> {
  if (response.ok) return;
  const detail = await response.text().catch(() => "");
  throw new Error(
    `Provider request failed (HTTP ${response.status}): ${detail || response.statusText}`,
  );
}

function emptyAssistant(model: Model): AssistantMessage {
  return {
    role: "assistant",
    content: [],
    api: model.api,
    provider: model.provider,
    model: model.id,
    usage: emptyUsage(),
    stopReason: "pending",
    timestamp: Date.now(),
  };
}

function emptyUsage(): Usage {
  return {
    input: 0,
    output: 0,
    cacheRead: 0,
    cacheWrite: 0,
    totalTokens: 0,
    cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, total: 0 },
  };
}

function errorEvent(
  model: Model,
  error: unknown,
  signal?: AbortSignal,
  output: AssistantMessage = emptyAssistant(model),
): Extract<AssistantMessageEvent, { type: "error" }> {
  const aborted = signal?.aborted === true;
  output.stopReason = aborted ? "aborted" : "error";
  output.errorMessage = error instanceof Error ? error.message : String(error);
  return { type: "error", reason: aborted ? "aborted" : "error", error: output };
}

function providerErrorMessage(value: unknown): string {
  const error = record(value);
  const code = string(error?.["code"]) ?? string(error?.["type"]);
  const message = string(error?.["message"]) ?? string(error?.["error"]);
  if (code !== undefined && message !== undefined) return `${code}: ${message}`;
  if (message !== undefined) return message;
  return JSON.stringify(value);
}

function appendText(output: AssistantMessage, delta: string): void {
  const current = output.content.at(-1);
  if (current?.type === "text") current.text += delta;
  else output.content.push({ type: "text", text: delta });
}

function appendThinking(output: AssistantMessage, delta: string): void {
  const current = output.content.at(-1);
  if (current?.type === "thinking") current.thinking += delta;
  else output.content.push({ type: "thinking", thinking: delta });
}

function parsedToolCall(call: { id: string; name: string; arguments: string }): ToolCall {
  let args: Record<string, unknown> = {};
  try {
    const parsed: unknown = JSON.parse(call.arguments || "{}");
    if (record(parsed) !== undefined) args = parsed as Record<string, unknown>;
  } catch {
    try {
      const repaired: unknown = JSON.parse(call.arguments.replaceAll(/,\s*([}\]])/gu, "$1"));
      if (record(repaired) !== undefined) args = repaired as Record<string, unknown>;
    } catch {
      // Preserve the raw provider payload while exposing no invented arguments.
    }
  }
  return {
    type: "toolCall",
    id: call.id,
    name: call.name,
    arguments: args,
    argumentsRaw: call.arguments,
  };
}

function openAIUsage(value: Record<string, unknown>): Usage {
  const input = number(value["prompt_tokens"]) ?? 0;
  const output = number(value["completion_tokens"]) ?? 0;
  const details = record(value["prompt_tokens_details"]);
  const cacheRead = number(details?.["cached_tokens"]) ?? 0;
  return {
    ...emptyUsage(),
    input,
    output,
    cacheRead,
    totalTokens: number(value["total_tokens"]) ?? input + output,
  };
}

function responsesUsage(value: Record<string, unknown>): Usage {
  const input = number(value["input_tokens"]) ?? 0;
  const output = number(value["output_tokens"]) ?? 0;
  const details = record(value["input_tokens_details"]);
  const cacheRead = number(details?.["cached_tokens"]) ?? 0;
  return {
    ...emptyUsage(),
    input,
    output,
    cacheRead,
    totalTokens: number(value["total_tokens"]) ?? input + output,
  };
}

function anthropicUsage(value: Record<string, unknown>): Usage {
  const input = number(value["input_tokens"]) ?? 0;
  const cacheRead = number(value["cache_read_input_tokens"]) ?? 0;
  const cacheWrite = number(value["cache_creation_input_tokens"]) ?? 0;
  const output = number(value["output_tokens"]) ?? 0;
  return {
    ...emptyUsage(),
    input,
    output,
    cacheRead,
    cacheWrite,
    totalTokens: input + output + cacheRead + cacheWrite,
  };
}

function thinkingBudget(level: string): number {
  const budgets: Record<string, number> = {
    minimal: 1_024,
    low: 2_048,
    medium: 4_096,
    high: 8_192,
    xhigh: 16_384,
    max: 32_768,
  };
  return budgets[level] ?? 4_096;
}

function accountIdFromJwt(token: string): string | undefined {
  try {
    const payload = token.split(".")[1];
    if (payload === undefined) return undefined;
    const json = JSON.parse(Buffer.from(payload, "base64url").toString("utf8")) as Record<
      string,
      unknown
    >;
    const auth = record(json["https://api.openai.com/auth"]);
    return string(auth?.["chatgpt_account_id"]);
  } catch {
    return undefined;
  }
}

function headersRecord(headers: Headers): Record<string, string> {
  return Object.fromEntries(headers.entries());
}

function trimSlash(value: string): string {
  return value.replace(/\/+$/u, "");
}

function record(value: unknown): Record<string, unknown> | undefined {
  return typeof value === "object" && value !== null && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : undefined;
}

function array(value: unknown): readonly unknown[] {
  return Array.isArray(value) ? value : [];
}

function firstRecord(value: unknown): Record<string, unknown> | undefined {
  return record(array(value)[0]);
}

function string(value: unknown): string | undefined {
  return typeof value === "string" ? value : undefined;
}

function number(value: unknown): number | undefined {
  return typeof value === "number" && Number.isFinite(value) ? value : undefined;
}
