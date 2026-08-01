import type {
  AssistantMessage,
  AssistantMessageEvent,
  AuthResult,
  Context,
  Model,
  ModelsSimpleStreamOptions,
  Message,
  ProviderHeaders,
  ImageContent,
  TextContent,
  ToolResultMessage,
  ToolCall,
  Usage,
} from "./types";
import {
  APIContextOverflowError,
  APIRequestTooLargeError,
  APIStatusError,
} from "#/llmProtocol/errors";
import { createAdapterRegistry } from "./adapterRegistry";
import { authenticateProviderRequest } from "./requestAuth";

const adapters = createAdapterRegistry({
  "openai-completions": streamOpenAIChat,
  "mistral-conversations": streamMistral,
  "openai-responses": streamOpenAIResponses,
  "azure-openai-responses": streamOpenAIResponses,
  "openai-codex-responses": streamOpenAIResponses,
  "anthropic-messages": streamAnthropic,
  "bedrock-converse-stream": streamBedrock,
  "google-generative-ai": streamGoogle,
  "google-vertex": streamGoogle,
  "pi-messages": streamPiMessages,
});

export function supportsProviderApi(api: string): api is import("./types").KnownApi {
  return adapters.supports(api);
}

export function providerApis(): readonly import("./types").KnownApi[] {
  return adapters.apis();
}

export function streamProvider(
  model: Model,
  context: Context,
  auth: AuthResult,
  options?: ModelsSimpleStreamOptions,
): AsyncIterable<AssistantMessageEvent> {
  return adapters.stream(model, context, auth, options) ??
    failedStream(model, `Unsupported provider API: ${model.api}`);
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
    const thinkingEffort = resolveThinking(model, options?.reasoning);
    const compat = openAICompletionsCompat(model);
    const cacheKey = clampCacheKey(options?.sessionId ?? options?.cacheKey);
    const response = await fetch(
      `${trimSlash(auth.auth.baseUrl ?? model.baseUrl)}/chat/completions`,
      {
        method: "POST",
        headers: protocolRequestHeaders(model, context, auth, {
          accept: "text/event-stream",
          "content-type": "application/json",
          ...sessionAffinityHeaders(compat, cacheKey),
        }),
        body: JSON.stringify({
          model: model.id,
          stream: true,
          stream_options: compat["supportsUsageInStreaming"] === false ? undefined : { include_usage: true },
          store: compat["supportsStore"] === true ? false : undefined,
          messages: toOpenAIChatMessages(model, context, compat),
          tools:
            context.tools?.map((tool) => ({
              type: "function",
              function: {
                name: tool.name,
                description: tool.description,
                parameters: tool.parameters,
                strict: compat["supportsStrictMode"] === false ? undefined : false,
              },
            })) ?? undefined,
          temperature: options?.temperature,
          top_p: options?.topP,
          max_tokens: compat["maxTokensField"] === "max_completion_tokens" ? undefined : options?.maxTokens,
          max_completion_tokens: compat["maxTokensField"] === "max_completion_tokens" ? options?.maxTokens : undefined,
          prompt_cache_key: cacheKey,
          ...openAIChatReasoningFields(model, compat, thinkingEffort),
          provider: record(compat["openRouterRouting"]),
          providerOptions: vercelGatewayOptions(compat),
          tool_stream: compat["zaiToolStream"] === true && context.tools?.length ? true : undefined,
          chat_template_kwargs: chatTemplateKwargs(compat, thinkingEffort),
          response_format: openAIResponseFormat(options?.responseFormat),
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
      const thinkingField = ["reasoning_content", "reasoning", "reasoning_text"] as const;
      const reasoningField = thinkingField.find((field) => string(delta?.[field]) !== undefined);
      const thinking = reasoningField === undefined ? undefined : string(delta?.[reasoningField]);
      if (thinking !== undefined) {
        const block = appendThinking(output, thinking);
        block.thinkingSignature ??= reasoningField;
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
    output.rawStopReason = finishReason;
    const stop = openAIChatStopReason(finishReason, calls.size > 0);
    if (stop.errorMessage !== undefined) throw new Error(stop.errorMessage);
    for (const call of calls.values()) {
      const toolCall = parsedToolCall(call);
      output.content.push(toolCall);
      yield { type: "toolcall_end", toolCall, partial: output };
    }
    output.stopReason = stop.stopReason;
    output.finishReason = normalizedFinishReason(output.stopReason, finishReason);
    yield {
      type: "done",
      reason: output.stopReason,
      message: output,
    };
  } catch (error) {
    yield errorEvent(model, error, options?.signal, output);
  }
}

/** Mistral chat has its own request contract even though its deltas are SSE-shaped. */
async function* streamMistral(
  model: Model,
  context: Context,
  auth: AuthResult,
  options?: ModelsSimpleStreamOptions,
): AsyncIterable<AssistantMessageEvent> {
  const output = emptyAssistant(model);
  yield { type: "start", partial: output };
  try {
    const thinking = resolveThinking(model, options?.reasoning);
    const response = await fetch(
      `${trimSlash(auth.auth.baseUrl ?? model.baseUrl)}/chat/completions`,
      {
        method: "POST",
        headers: requestHeaders(model.headers, auth, {
          accept: "text/event-stream",
          "content-type": "application/json",
          ...(options?.cacheKey === undefined ? undefined : { "x-affinity": clampCacheKey(options.cacheKey)! }),
        }),
        body: JSON.stringify({
          model: model.id,
          stream: true,
          messages: toMistralMessages(context),
          tools:
            context.tools?.map((tool) => ({
              type: "function",
              function: {
                name: tool.name,
                description: tool.description,
                parameters: tool.parameters,
                strict: true,
              },
            })) ?? undefined,
          temperature: options?.temperature,
          top_p: options?.topP,
          max_tokens: options?.maxTokens,
          prompt_mode: thinking === undefined ? undefined : "reasoning",
          reasoning_effort:
            typeof mistralReasoningEffort(model, thinking) === "string"
              ? mistralReasoningEffort(model, thinking)
              : undefined,
          response_format: openAIResponseFormat(options?.responseFormat),
        }),
        signal: options?.signal,
      },
    );
    const responseHeaders = headersRecord(response.headers);
    output.traceId = responseHeaders["x-request-id"] ?? responseHeaders["x-trace-id"];
    options?.onResponse?.({ headers: responseHeaders });
    await assertOk(response);

    const calls = new Map<number, { id: string; name: string; arguments: string; started: boolean }>();
    let finishReason: string | undefined;
    for await (const data of sseJson(response)) {
      const choice = firstRecord(data["choices"]);
      const delta = record(choice?.["delta"]);
      for (const item of mistralContentDeltas(delta?.["content"])) {
        if (item.type === "thinking") {
          appendThinking(output, item.text);
          yield { type: "thinking_delta", delta: item.text, partial: output };
        } else {
          appendText(output, item.text);
          yield { type: "text_delta", delta: item.text, partial: output };
        }
      }
      for (const raw of array(delta?.["tool_calls"])) {
        const item = record(raw);
        const index = number(item?.["index"]) ?? calls.size;
        const fn = record(item?.["function"]);
        const call = calls.get(index) ?? { id: "", name: "", arguments: "", started: false };
        call.id += string(item?.["id"]) ?? "";
        call.name += string(fn?.["name"]) ?? "";
        const argumentsDelta = string(fn?.["arguments"]) ?? "";
        call.arguments += argumentsDelta;
        calls.set(index, call);
        if (!call.started && (call.id !== "" || call.name !== "")) {
          call.started = true;
          yield { type: "toolcall_start", index, id: call.id, name: call.name, partial: output };
        }
        if (argumentsDelta !== "") {
          yield { type: "toolcall_delta", index, delta: argumentsDelta, partial: output };
        }
      }
      finishReason = string(choice?.["finish_reason"]) ?? finishReason;
      const usage = record(data["usage"]);
      if (usage !== undefined) output.usage = mistralUsage(usage);
      output.responseId = string(data["id"]) ?? output.responseId;
      output.responseModel = string(data["model"]) ?? output.responseModel;
    }
    output.rawStopReason = finishReason;
    if (finishReason === undefined) {
      throw new Error("Mistral stream ended without a finish reason");
    }
    if (finishReason === "error" || !isMistralFinishReason(finishReason)) {
      throw new Error(`Provider stopped with: ${finishReason}`);
    }
    for (const call of calls.values()) {
      const toolCall = parsedToolCall(call);
      output.content.push(toolCall);
      yield { type: "toolcall_end", toolCall, partial: output };
    }
    output.stopReason =
      calls.size > 0 || finishReason === "tool_calls"
        ? "toolUse"
        : finishReason === "length" || finishReason === "model_length"
          ? "length"
          : "stop";
    output.finishReason = normalizedFinishReason(output.stopReason, finishReason);
    yield { type: "done", reason: output.stopReason, message: output };
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
    const azure = model.api === "azure-openai-responses";
    const endpoint = codex
      ? `${trimSlash(auth.auth.baseUrl ?? model.baseUrl)}/codex/responses`
      : azure
        ? azureResponsesEndpoint(auth.auth.baseUrl ?? model.baseUrl)
        : `${trimSlash(auth.auth.baseUrl ?? model.baseUrl)}/responses`;
    const token = auth.auth.apiKey;
    const accountId = token === undefined ? undefined : accountIdFromJwt(token);
    const sessionId = clampCacheKey(options?.sessionId ?? options?.cacheKey);
    const thinkingEffort = resolveThinking(model, options?.reasoning);
    const compat = openAIResponsesCompat(model);
    const headers = protocolRequestHeaders(model, context, auth, {
      accept: "text/event-stream",
      "content-type": "application/json",
      ...(codex
        ? {
            originator: "dimi",
            "openai-beta": "responses=experimental",
            ...(accountId === undefined ? {} : { "chatgpt-account-id": accountId }),
          }
        : {}),
      ...(azure && auth.auth.apiKey !== undefined ? { "api-key": auth.auth.apiKey } : {}),
      ...(codex
        ? codexSessionHeaders(sessionId)
        : responsesSessionAffinityHeaders(compat, sessionId)),
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
            strict: compat["supportsStrictMode"] === false ? undefined : false,
          })) ?? undefined,
        parallel_tool_calls: true,
        temperature: options?.temperature,
        top_p: options?.topP,
        max_output_tokens: options?.maxTokens,
        prompt_cache_key: clampCacheKey(options?.cacheKey),
        text: openAIResponsesTextFormat(options?.responseFormat),
        reasoning:
          responseReasoningEffort(model, compat, thinkingEffort) === undefined
            ? undefined
            : { effort: responseReasoningEffort(model, compat, thinkingEffort), summary: "auto" },
        provider: record(compat["openRouterRouting"]),
        include:
          options?.reasoning === undefined && !contextHasResponsesReasoning(context)
            ? undefined
            : ["reasoning.encrypted_content"],
      }),
      signal: options?.signal,
    });
    const responseHeaders = headersRecord(response.headers);
    output.traceId = responseHeaders["x-trace-id"];
    options?.onResponse?.({ headers: responseHeaders });
    await assertOk(response);

    const calls = new Map<string, { id: string; name: string; arguments: string }>();
    const thinking = new Map<
      string,
      Extract<AssistantMessage["content"][number], { type: "thinking" }>
    >();
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
        const itemId = string(data["item_id"]);
        const block = itemId === undefined ? undefined : thinking.get(itemId);
        if (block === undefined) appendThinking(output, delta);
        else block.thinking += delta;
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
        } else if (item?.["type"] === "reasoning") {
          const id = string(item["id"]);
          if (id !== undefined) {
            const block = { type: "thinking" as const, thinking: "", itemId: id };
            output.content.push(block);
            thinking.set(id, block);
          }
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
        } else if (item?.["type"] === "reasoning") {
          const id = string(item["id"]);
          const block = id === undefined ? undefined : thinking.get(id);
          if (block !== undefined) {
            block.thinkingSignature = string(item["encrypted_content"]);
            block.reasoningItem = { ...item };
            const summary = array(item["summary"])
              .map((entry) => string(record(entry)?.["text"]) ?? "")
              .join("");
            if (block.thinking.length === 0) block.thinking = summary;
          }
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
    const thinking = resolveThinking(model, options?.reasoning);
    const compat = compatRecord(model);
    const response = await fetch(`${trimSlash(auth.auth.baseUrl ?? model.baseUrl)}/messages`, {
      method: "POST",
      headers: protocolRequestHeaders(model, context, auth, {
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
        temperature: compat["supportsTemperature"] === false ? undefined : options?.temperature,
        top_p: options?.topP,
        max_tokens: options?.maxTokens ?? model.maxTokens,
        metadata:
          options?.cacheKey === undefined ? undefined : { user_id: clampCacheKey(options.cacheKey) },
        output_config: anthropicResponseFormat(options?.responseFormat),
        context_management:
          thinking === undefined ? undefined : anthropicThinkingKeep(options?.thinkingKeep),
        thinking:
          thinking === undefined
            ? undefined
            : {
                type: "enabled",
                budget_tokens: Math.min(
                  thinkingBudgetFromValue(thinking),
                  Math.max(1_024, (options?.maxTokens ?? model.maxTokens) - 1),
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

/**
 * Bedrock ConverseStream uses AWS EventStream frames rather than SSE. This
 * adapter deliberately speaks the documented wire protocol so it also works
 * with compatible gateways and Bedrock bearer-token authentication.
 */
async function* streamBedrock(
  model: Model,
  context: Context,
  auth: AuthResult,
  options?: ModelsSimpleStreamOptions,
): AsyncIterable<AssistantMessageEvent> {
  const output = emptyAssistant(model);
  yield { type: "start", partial: output };
  try {
    const url = `${trimSlash(auth.auth.baseUrl ?? model.baseUrl)}/model/${encodeURIComponent(model.id)}/converse-stream`;
    const body = JSON.stringify(bedrockRequest(model, context, options));
    const request = await authenticateProviderRequest(
      model,
      auth,
      url,
      requestHeaders(model.headers, auth, {
        accept: "application/vnd.amazon.eventstream",
        "content-type": "application/json",
        "x-amzn-bedrock-accept": "application/json",
      }),
      body,
    );
    const response = await fetch(request.url, {
      method: "POST",
      headers: request.headers,
      body,
      signal: options?.signal,
    });
    const responseHeaders = headersRecord(response.headers);
    output.traceId = responseHeaders["x-amzn-requestid"] ?? responseHeaders["x-amz-request-id"];
    options?.onResponse?.({ headers: responseHeaders });
    await assertOk(response);

    const blocks = new Map<number, AssistantMessage["content"][number]>();
    let terminal = false;
    for await (const event of bedrockEvents(response)) {
      const payload = event.payload;
      if (event.type === "contentBlockStart") {
        const index = number(payload["contentBlockIndex"]) ?? blocks.size;
        const start = record(payload["start"]);
        const use = record(start?.["toolUse"]);
        if (use !== undefined) {
          const call = {
            type: "toolCall" as const,
            id: string(use["toolUseId"]) ?? "",
            name: string(use["name"]) ?? "",
            arguments: {},
            argumentsRaw: "",
          };
          blocks.set(index, call);
          output.content.push(call);
          yield {
            type: "toolcall_start",
            index,
            id: call.id,
            name: call.name,
            partial: output,
          };
        }
      } else if (event.type === "contentBlockDelta") {
        const index = number(payload["contentBlockIndex"]) ?? 0;
        const delta = record(payload["delta"]);
        const text = string(delta?.["text"]);
        if (text !== undefined) {
          const block = bedrockTextBlock(output, blocks, index);
          block.text += text;
          yield { type: "text_delta", delta: text, partial: output };
          continue;
        }
        const tool = record(delta?.["toolUse"]);
        if (tool !== undefined) {
          const call = blocks.get(index);
          const deltaText = string(tool["input"]) ?? "";
          if (call?.type === "toolCall") {
            call.argumentsRaw = `${call.argumentsRaw ?? ""}${deltaText}`;
            call.arguments = parseJsonObject(call.argumentsRaw);
          }
          if (deltaText !== "") {
            yield { type: "toolcall_delta", index, delta: deltaText, partial: output };
          }
          continue;
        }
        const reasoning = record(delta?.["reasoningContent"]);
        const reasoningText = record(reasoning?.["reasoningText"]);
        const thought = string(reasoningText?.["text"]);
        if (thought !== undefined) {
          const block = bedrockThinkingBlock(output, blocks, index);
          block.thinking += thought;
          block.thinkingSignature = string(reasoningText?.["signature"]) ?? block.thinkingSignature;
          yield { type: "thinking_delta", delta: thought, partial: output };
        }
      } else if (event.type === "contentBlockStop") {
        const index = number(payload["contentBlockIndex"]) ?? 0;
        const call = blocks.get(index);
        if (call?.type === "toolCall") {
          call.arguments = parseJsonObject(call.argumentsRaw ?? "");
          yield { type: "toolcall_end", toolCall: call, partial: output };
        }
      } else if (event.type === "messageStop") {
        output.rawStopReason = string(payload["stopReason"]);
        terminal = true;
      } else if (event.type === "metadata") {
        const usage = record(payload["usage"]);
        if (usage !== undefined) output.usage = bedrockUsage(usage);
      } else if (event.type.endsWith("Exception")) {
        throw new Error(providerErrorMessage(payload));
      }
    }
    if (!terminal) throw new Error("Bedrock stream ended before a message stop event");
    const stop = bedrockStopReason(output.rawStopReason);
    if (stop.errorMessage !== undefined) throw new Error(stop.errorMessage);
    output.stopReason = stop.stopReason;
    output.finishReason = normalizedFinishReason(output.stopReason, output.rawStopReason);
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

async function* streamGoogle(
  model: Model,
  context: Context,
  auth: AuthResult,
  options?: ModelsSimpleStreamOptions,
): AsyncIterable<AssistantMessageEvent> {
  const output = emptyAssistant(model);
  yield { type: "start", partial: output };
  try {
    const url = `${trimSlash(auth.auth.baseUrl ?? model.baseUrl)}${
      model.api === "google-vertex" ? "/publishers/google/models/" : "/models/"
    }${encodeURIComponent(model.id)}:streamGenerateContent?alt=sse`;
    const body = JSON.stringify(googleRequest(model, context, options));
    const request = await authenticateProviderRequest(
      model,
      auth,
      url,
      requestHeaders(model.headers, auth, {
        accept: "text/event-stream",
        "content-type": "application/json",
        ...((model.api === "google-generative-ai" || model.api === "google-vertex") && auth.auth.apiKey !== undefined
          ? { "x-goog-api-key": auth.auth.apiKey }
          : {}),
      }),
      body,
    );
    const response = await fetch(request.url, {
      method: "POST",
      headers: request.headers,
      body,
      signal: options?.signal,
    });
    const responseHeaders = headersRecord(response.headers);
    output.traceId = responseHeaders["x-trace-id"];
    options?.onResponse?.({ headers: responseHeaders });
    await assertOk(response);
    const calls = new Map<string, ToolCall>();
    for await (const data of sseJson(response)) {
      output.responseId ??= string(data["responseId"]);
      const candidate = record(array(data["candidates"])[0]);
      const parts = array(record(candidate?.["content"])?.["parts"]);
      for (const rawPart of parts) {
        const part = record(rawPart);
        const text = string(part?.["text"]);
        const thought = part?.["thought"] === true;
        if (text !== undefined) {
          if (thought) {
            const block = appendThinking(output, text);
            block.thinkingSignature ??= string(part?.["thoughtSignature"]);
            yield { type: "thinking_delta", delta: text, partial: output };
          } else {
            appendText(output, text);
            yield { type: "text_delta", delta: text, partial: output };
          }
        }
        const call = record(part?.["functionCall"]);
        if (call !== undefined) {
          const id = string(call["id"]) ?? `${string(call["name"]) ?? "tool"}-${calls.size}`;
          const toolCall: ToolCall = {
            type: "toolCall",
            id,
            name: string(call["name"]) ?? "",
            arguments: record(call["args"]) ?? {},
            thoughtSignature: string(part?.["thoughtSignature"]),
          };
          calls.set(id, toolCall);
          yield { type: "toolcall_start", index: id, id, name: toolCall.name, partial: output };
          yield {
            type: "toolcall_delta",
            index: id,
            delta: JSON.stringify(toolCall.arguments),
            partial: output,
          };
        }
      }
      const finish = string(candidate?.["finishReason"]);
      if (finish !== undefined) output.rawStopReason = finish;
      const usage = record(data["usageMetadata"]);
      if (usage !== undefined) output.usage = googleUsage(usage);
    }
    for (const call of calls.values()) {
      output.content.push(call);
      yield { type: "toolcall_end", toolCall: call, partial: output };
    }
    if (output.rawStopReason === undefined) {
      throw new Error("Google stream ended without a finish reason");
    }
    const stop = googleStopReason(output.rawStopReason);
    if (stop === "error") throw new Error(`Provider stopped with: ${output.rawStopReason}`);
    output.stopReason = calls.size > 0 ? "toolUse" : stop;
    output.finishReason = normalizedFinishReason(output.stopReason, output.rawStopReason);
    yield { type: "done", reason: output.stopReason, message: output };
  } catch (error) {
    yield errorEvent(model, error, options?.signal, output);
  }
}

function googleRequest(
  model: Model,
  context: Context,
  options?: ModelsSimpleStreamOptions,
): Record<string, unknown> {
  const thinking = resolveThinking(model, options?.reasoning);
  const contents = googleContents(context.messages);
  const config: Record<string, unknown> = {
    temperature: options?.temperature,
    topP: options?.topP,
    maxOutputTokens: options?.maxTokens,
    responseMimeType:
      options?.responseFormat === undefined || options.responseFormat.type === "text"
        ? undefined
        : "application/json",
    responseJsonSchema:
      options?.responseFormat?.type === "json_schema" ? options.responseFormat.jsonSchema?.schema : undefined,
    thinkingConfig:
      thinking === undefined
        ? undefined
        : typeof thinking === "number"
          ? { includeThoughts: true, thinkingBudget: thinking }
          : { includeThoughts: true, thinkingLevel: thinking.toUpperCase() },
  };
  return {
    contents,
    systemInstruction:
      context.systemPrompt === undefined ? undefined : { parts: [{ text: context.systemPrompt }] },
    tools:
      context.tools === undefined || context.tools.length === 0
        ? undefined
        : [{ functionDeclarations: context.tools.map((tool) => ({ name: tool.name, description: tool.description, parameters: tool.parameters })) }],
    generationConfig: Object.fromEntries(
      Object.entries(config).filter(([, value]) => value !== undefined),
    ),
  };
}

function googleContents(messages: readonly Message[]): unknown[] {
  const contents: unknown[] = [];
  for (let index = 0; index < messages.length; index += 1) {
    const message = messages[index]!;
    if (message.role === "user") {
      contents.push({
        role: "user",
        parts:
          typeof message.content === "string"
            ? [{ text: message.content }]
            : message.content.map((part) =>
                part.type === "text"
                  ? { text: part.text }
                  : part.url === undefined
                    ? { inlineData: { mimeType: part.mimeType, data: part.data } }
                    : { fileData: { mimeType: part.mimeType, fileUri: part.url } },
              ),
      });
      continue;
    }
    if (message.role === "assistant") {
      contents.push({
        role: "model",
        parts: message.content.map((part) => {
          if (part.type === "text") return { text: part.text };
          if (part.type === "thinking") {
            return { text: part.thinking, thought: true, thoughtSignature: part.thinkingSignature };
          }
          return {
            functionCall: { name: part.name, args: part.arguments, id: part.id },
            thoughtSignature: part.thoughtSignature,
          };
        }),
      });
      continue;
    }
    const results: ToolResultMessage[] = [message];
    while (messages[index + 1]?.role === "toolResult") results.push(messages[++index] as ToolResultMessage);
    contents.push({
      role: "user",
      parts: results.map((result) => ({
        functionResponse: {
          name: result.toolName,
          id: result.toolCallId,
          response: {
            content: result.content
              .filter((part): part is TextContent => part.type === "text")
              .map((part) => part.text)
              .join("\n"),
          },
          parts: result.content
            .filter((part): part is ImageContent => part.type === "image")
            .map((part) =>
              part.url === undefined
                ? { inlineData: { mimeType: part.mimeType, data: part.data } }
                : { fileData: { mimeType: part.mimeType, fileUri: part.url } },
            ),
        },
      })),
    });
  }
  return contents;
}

function googleUsage(raw: Record<string, unknown>): Usage {
  const cacheRead = number(raw["cachedContentTokenCount"]) ?? 0;
  const input = (number(raw["promptTokenCount"]) ?? 0) - cacheRead;
  const reasoning = number(raw["thoughtsTokenCount"]) ?? 0;
  const output = (number(raw["candidatesTokenCount"]) ?? 0) + reasoning;
  return {
    input,
    output,
    cacheRead,
    cacheWrite: 0,
    reasoning,
    totalTokens: number(raw["totalTokenCount"]) ?? input + output + cacheRead,
    cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, total: 0 },
  };
}

async function* streamPiMessages(
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
      }),
      body: JSON.stringify({
        model: model.id,
        context,
        options: {
          temperature: options?.temperature,
          topP: options?.topP,
          maxTokens: options?.maxTokens,
          reasoning: resolveThinking(model, options?.reasoning),
          thinkingKeep:
            resolveThinking(model, options?.reasoning) === undefined ? undefined : options?.thinkingKeep,
          sessionId: options?.cacheKey,
          responseFormat: options?.responseFormat,
        },
      }),
      signal: options?.signal,
    });
    const responseHeaders = headersRecord(response.headers);
    output.traceId = responseHeaders["x-trace-id"];
    options?.onResponse?.({ headers: responseHeaders });
    await assertOk(response);
    const calls = new Map<number, ToolCall>();
    for await (const data of sseJson(response)) {
      const type = string(data["type"]);
      const contentIndex = number(data["contentIndex"]);
      if (type === "text_delta") {
        const delta = string(data["delta"]) ?? "";
        const block = piTextBlock(output, contentIndex);
        block.text += delta;
        yield { type: "text_delta", delta, partial: output };
      } else if (type === "thinking_delta") {
        const delta = string(data["delta"]) ?? "";
        const block = piThinkingBlock(output, contentIndex);
        block.thinking += delta;
        yield { type: "thinking_delta", delta, partial: output };
      } else if (type === "thinking_end") {
        const block = piThinkingBlock(output, contentIndex);
        block.thinking = string(data["content"]) ?? block.thinking;
        block.thinkingSignature = string(data["contentSignature"]);
      } else if (type === "toolcall_start") {
        const index = contentIndex ?? calls.size;
        const call: ToolCall = {
          type: "toolCall",
          id: string(data["id"]) ?? "",
          name: string(data["toolName"]) ?? string(data["name"]) ?? "",
          arguments: {},
          argumentsRaw: "",
        };
        calls.set(index, call);
        output.content[index] = call;
        yield { type: "toolcall_start", index, id: call.id, name: call.name, partial: output };
      } else if (type === "toolcall_delta") {
        const index = contentIndex ?? 0;
        const call = calls.get(index);
        const delta = string(data["delta"]) ?? "";
        if (call !== undefined) call.argumentsRaw = `${call.argumentsRaw ?? ""}${delta}`;
        yield { type: "toolcall_delta", index, delta, partial: output };
      } else if (type === "toolcall_end") {
        const index = contentIndex ?? 0;
        const eventCall = record(data["toolCall"]);
        const call = calls.get(index);
        const toolCall = {
          type: "toolCall" as const,
          id: string(eventCall?.["id"]) ?? call?.id ?? "",
          name: string(eventCall?.["name"]) ?? call?.name ?? "",
          arguments: record(eventCall?.["arguments"]) ?? call?.arguments ?? {},
          argumentsRaw: string(eventCall?.["argumentsRaw"]) ?? call?.argumentsRaw,
          thoughtSignature: string(eventCall?.["thoughtSignature"]),
        };
        output.content[index] = toolCall;
        calls.set(index, toolCall);
        yield { type: "toolcall_end", toolCall, partial: output };
      } else if (type === "done") {
        const usage = record(data["usage"]);
        if (usage !== undefined) output.usage = piUsage(usage);
        output.responseId = string(data["responseId"]);
        output.stopReason = piStopReason(string(data["reason"]));
        output.finishReason = normalizedFinishReason(output.stopReason, string(data["reason"]));
        yield { type: "done", reason: output.stopReason, message: output };
        return;
      } else if (type === "error") {
        const usage = record(data["usage"]);
        if (usage !== undefined) output.usage = piUsage(usage);
        output.errorMessage = string(data["errorMessage"]);
        throw new Error(output.errorMessage ?? "Pi Messages provider error");
      }
    }
    throw new Error("Pi Messages stream ended before a terminal event");
  } catch (error) {
    yield errorEvent(model, error, options?.signal, output);
  }
}

function piTextBlock(
  output: AssistantMessage,
  index: number | undefined,
): Extract<AssistantMessage["content"][number], { type: "text" }> {
  const existing = index === undefined ? output.content.at(-1) : output.content[index];
  if (existing?.type === "text") return existing;
  const block = { type: "text" as const, text: "" };
  if (index === undefined) output.content.push(block);
  else output.content[index] = block;
  return block;
}

function piThinkingBlock(
  output: AssistantMessage,
  index: number | undefined,
): Extract<AssistantMessage["content"][number], { type: "thinking" }> {
  const existing = index === undefined ? output.content.at(-1) : output.content[index];
  if (existing?.type === "thinking") return existing;
  const block = { type: "thinking" as const, thinking: "" };
  if (index === undefined) output.content.push(block);
  else output.content[index] = block;
  return block;
}

function piUsage(raw: Record<string, unknown>): Usage {
  return {
    input: number(raw["input"]) ?? 0,
    output: number(raw["output"]) ?? 0,
    cacheRead: number(raw["cacheRead"]) ?? 0,
    cacheWrite: number(raw["cacheWrite"]) ?? 0,
    reasoning: number(raw["reasoning"]),
    totalTokens: number(raw["totalTokens"]) ?? 0,
    cost: {
      input: number(record(raw["cost"])?.["input"]) ?? 0,
      output: number(record(raw["cost"])?.["output"]) ?? 0,
      cacheRead: number(record(raw["cost"])?.["cacheRead"]) ?? 0,
      cacheWrite: number(record(raw["cost"])?.["cacheWrite"]) ?? 0,
      total: number(record(raw["cost"])?.["total"]) ?? 0,
    },
  };
}

function piStopReason(value: string | undefined): "stop" | "length" | "toolUse" {
  return value === "length" ? "length" : value === "toolUse" ? "toolUse" : "stop";
}

function bedrockRequest(
  model: Model,
  context: Context,
  options?: ModelsSimpleStreamOptions,
): Record<string, unknown> {
  const thinking = resolveThinking(model, options?.reasoning);
  return {
    messages: bedrockMessages(context.messages),
    system:
      context.systemPrompt === undefined ? undefined : [{ text: context.systemPrompt }],
    inferenceConfig: {
      maxTokens: options?.maxTokens,
      temperature: options?.temperature,
      topP: options?.topP,
    },
    toolConfig:
      context.tools === undefined || context.tools.length === 0
        ? undefined
        : {
            tools: context.tools.map((tool) => ({
              toolSpec: {
                name: tool.name,
                description: tool.description,
                inputSchema: { json: tool.parameters },
              },
            })),
          },
    additionalModelRequestFields:
      thinking === undefined
        ? undefined
        : {
            thinking: {
              type: "enabled",
              budget_tokens: thinkingBudgetFromValue(thinking),
            },
          },
  };
}

function bedrockMessages(messages: readonly Message[]): unknown[] {
  const result: unknown[] = [];
  for (let index = 0; index < messages.length; index += 1) {
    const message = messages[index]!;
    if (message.role === "user") {
      result.push({
        role: "user",
        content:
          typeof message.content === "string"
            ? [{ text: message.content }]
            : message.content.map((part) =>
                part.type === "text" ? { text: part.text } : bedrockImage(part),
              ),
      });
      continue;
    }
    if (message.role === "assistant") {
      const content: Record<string, unknown>[] = [];
      for (const part of message.content) {
        if (part.type === "text") {
          if (part.text.length > 0) content.push({ text: part.text });
          continue;
        }
        if (part.type === "toolCall") {
          content.push({ toolUse: { toolUseId: part.id, name: part.name, input: part.arguments } });
          continue;
        }
        if (part.thinking.length === 0) continue;
        if (part.thinkingSignature === undefined) content.push({ text: part.thinking });
        else {
          content.push({
            reasoningContent: {
              reasoningText: { text: part.thinking, signature: part.thinkingSignature },
            },
          });
        }
      }
      if (content.length > 0) result.push({ role: "assistant", content });
      continue;
    }
    const toolResults: ToolResultMessage[] = [message];
    while (messages[index + 1]?.role === "toolResult") {
      toolResults.push(messages[++index] as ToolResultMessage);
    }
    result.push({
      role: "user",
      content: toolResults.map((toolResult) => ({
        toolResult: {
          toolUseId: toolResult.toolCallId,
          status: toolResult.isError ? "error" : "success",
          content: toolResult.content.map((part) =>
            part.type === "text" ? { text: part.text } : bedrockImage(part),
          ),
        },
      })),
    });
  }
  return result;
}

function bedrockImage(image: ImageContent): Record<string, unknown> {
  const mime = image.mimeType ?? "image/png";
  const format = mime === "image/jpeg" || mime === "image/jpg" ? "jpeg" : mime.split("/")[1] ?? "png";
  return {
    image: {
      format,
      source: image.url === undefined ? { bytes: image.data ?? "" } : { s3Location: { uri: image.url } },
    },
  };
}

async function* bedrockEvents(
  response: Response,
): AsyncIterable<{ type: string; payload: Record<string, unknown> }> {
  if (response.body === null) throw new Error("Provider response has no body");
  const reader = response.body.getReader();
  let buffer: Uint8Array<ArrayBufferLike> = new Uint8Array();
  while (true) {
    const { done, value } = await reader.read();
    if (value !== undefined) buffer = concatBytes(buffer, value);
    let offset = 0;
    while (buffer.length - offset >= 12) {
      const frameLength = new DataView(buffer.buffer, buffer.byteOffset + offset).getUint32(0);
      if (frameLength < 16) throw new Error("Invalid AWS EventStream frame length");
      if (buffer.length - offset < frameLength) break;
      const frame = buffer.subarray(offset, offset + frameLength);
      offset += frameLength;
      const headersLength = new DataView(frame.buffer, frame.byteOffset).getUint32(4);
      const headersEnd = 12 + headersLength;
      if (headersEnd > frame.length - 4) throw new Error("Invalid AWS EventStream header length");
      const headers = awsEventHeaders(frame.subarray(12, headersEnd));
      const payloadText = new TextDecoder().decode(frame.subarray(headersEnd, frame.length - 4));
      const payload = record(JSON.parse(payloadText));
      if (payload === undefined) continue;
      const type = headers.get(":event-type");
      if (type !== undefined) yield { type, payload };
    }
    buffer = buffer.subarray(offset);
    if (done) break;
  }
  if (buffer.length > 0) throw new Error("Truncated AWS EventStream frame");
}

function awsEventHeaders(bytes: Uint8Array): Map<string, string> {
  const result = new Map<string, string>();
  let offset = 0;
  while (offset < bytes.length) {
    const nameLength = bytes[offset++];
    if (nameLength === undefined || offset + nameLength + 1 > bytes.length) {
      throw new Error("Invalid AWS EventStream header");
    }
    const name = new TextDecoder().decode(bytes.subarray(offset, offset + nameLength));
    offset += nameLength;
    const type = bytes[offset++];
    if (type === 7) {
      if (offset + 2 > bytes.length) throw new Error("Invalid AWS EventStream string header");
      const length = new DataView(bytes.buffer, bytes.byteOffset + offset).getUint16(0);
      offset += 2;
      if (offset + length > bytes.length) throw new Error("Invalid AWS EventStream string header");
      result.set(name, new TextDecoder().decode(bytes.subarray(offset, offset + length)));
      offset += length;
    } else {
      offset += awsHeaderValueLength(type, bytes, offset);
    }
  }
  return result;
}

function awsHeaderValueLength(type: number | undefined, bytes: Uint8Array, offset: number): number {
  if (type === 0 || type === 1) return 0;
  if (type === 2) return 1;
  if (type === 3) return 2;
  if (type === 4) return 4;
  if (type === 5 || type === 8) return 8;
  if (type === 9) return 16;
  if (type === 6) {
    if (offset + 2 > bytes.length) throw new Error("Invalid AWS EventStream byte header");
    return 2 + new DataView(bytes.buffer, bytes.byteOffset + offset).getUint16(0);
  }
  throw new Error("Unsupported AWS EventStream header type");
}

function concatBytes(
  first: Uint8Array<ArrayBufferLike>,
  second: Uint8Array<ArrayBufferLike>,
): Uint8Array<ArrayBufferLike> {
  const result = new Uint8Array(first.length + second.length);
  result.set(first);
  result.set(second, first.length);
  return result;
}

function bedrockTextBlock(
  output: AssistantMessage,
  blocks: Map<number, AssistantMessage["content"][number]>,
  index: number,
): TextContent {
  const current = blocks.get(index);
  if (current?.type === "text") return current;
  const block = { type: "text" as const, text: "" };
  blocks.set(index, block);
  output.content.push(block);
  return block;
}

function bedrockThinkingBlock(
  output: AssistantMessage,
  blocks: Map<number, AssistantMessage["content"][number]>,
  index: number,
): Extract<AssistantMessage["content"][number], { type: "thinking" }> {
  const current = blocks.get(index);
  if (current?.type === "thinking") return current;
  const block = { type: "thinking" as const, thinking: "" };
  blocks.set(index, block);
  output.content.push(block);
  return block;
}

function bedrockUsage(value: Record<string, unknown>): Usage {
  const input = number(value["inputTokens"]) ?? 0;
  const output = number(value["outputTokens"]) ?? 0;
  const cacheRead = number(value["cacheReadInputTokens"]) ?? 0;
  const cacheWrite = number(value["cacheWriteInputTokens"]) ?? 0;
  return {
    ...emptyUsage(),
    input,
    output,
    cacheRead,
    cacheWrite,
    totalTokens: number(value["totalTokens"]) ?? input + output + cacheRead + cacheWrite,
  };
}

function bedrockStopReason(value: string | undefined): {
  stopReason: "stop" | "length" | "toolUse";
  errorMessage?: string;
} {
  if (value === "end_turn" || value === "stop_sequence") return { stopReason: "stop" };
  if (value === "max_tokens" || value === "model_context_window_exceeded") return { stopReason: "length" };
  if (value === "tool_use") return { stopReason: "toolUse" };
  return { stopReason: "stop", errorMessage: `Provider stopped with: ${value ?? "unknown"}` };
}

function googleStopReason(value: string): "stop" | "length" | "error" {
  if (value === "STOP") return "stop";
  if (value === "MAX_TOKENS") return "length";
  return "error";
}

function parseJsonObject(value: string): Record<string, unknown> {
  try {
    const parsed = JSON.parse(value || "{}");
    return record(parsed) ?? {};
  } catch {
    return {};
  }
}

function toMistralMessages(context: Context): unknown[] {
  const result: unknown[] = [];
  if (context.systemPrompt !== undefined) result.push({ role: "system", content: context.systemPrompt });
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
                  : { type: "image_url", image_url: imageUrl(part) },
              ),
      });
      continue;
    }
    if (message.role === "assistant") {
      const content: Record<string, unknown>[] = [];
      for (const part of message.content) {
        if (part.type === "text") {
          content.push({ type: "text", text: part.text });
          continue;
        }
        if (part.type === "thinking") {
          if (part.thinking.length > 0) {
            content.push({ type: "thinking", thinking: [{ type: "text", text: part.thinking }] });
          }
        }
      }
      const toolCalls = message.content
        .filter((part): part is ToolCall => part.type === "toolCall")
        .map((call) => ({
          id: mistralToolCallId(call.id),
          type: "function",
          function: { name: call.name, arguments: JSON.stringify(call.arguments) },
        }));
      if (content.length > 0 || toolCalls.length > 0) {
        result.push({
          role: "assistant",
          content: content.length === 0 ? undefined : content,
          tool_calls: toolCalls.length === 0 ? undefined : toolCalls,
        });
      }
      continue;
    }
    result.push({
      role: "tool",
      tool_call_id: mistralToolCallId(message.toolCallId),
      name: message.toolName,
      content: openAIChatToolResult(message.content),
    });
  }
  return result;
}

function mistralToolCallId(value: string): string {
  const normalized = value.replaceAll(/[^a-zA-Z0-9]/gu, "");
  if (normalized.length === 9) return normalized;
  let hash = 2_166_136_261;
  for (const char of value) hash = Math.imul(hash ^ char.codePointAt(0)!, 16_777_619);
  return `m${(hash >>> 0).toString(36)}`.padEnd(9, "0").slice(0, 9);
}

function mistralContentDeltas(value: unknown): { type: "text" | "thinking"; text: string }[] {
  if (typeof value === "string") return [{ type: "text", text: value }];
  const result: { type: "text" | "thinking"; text: string }[] = [];
  for (const entry of array(value)) {
    const item = record(entry);
    if (item?.["type"] === "thinking") {
      const text = array(item["thinking"])
        .map((part) => string(record(part)?.["text"]) ?? "")
        .join("");
      if (text !== "") result.push({ type: "thinking", text });
      continue;
    }
    const text = string(item?.["text"]);
    if (text !== undefined) result.push({ type: "text", text });
  }
  return result;
}

function mistralUsage(value: Record<string, unknown>): Usage {
  const input = number(value["prompt_tokens"]) ?? number(value["promptTokens"]) ?? 0;
  const output = number(value["completion_tokens"]) ?? number(value["completionTokens"]) ?? 0;
  const detail = record(value["prompt_tokens_details"]) ?? record(value["promptTokensDetails"]);
  const cacheRead = number(detail?.["cached_tokens"]) ?? number(detail?.["cachedTokens"]) ?? 0;
  return {
    ...emptyUsage(),
    input: Math.max(0, input - cacheRead),
    output,
    cacheRead,
    totalTokens: number(value["total_tokens"]) ?? number(value["totalTokens"]) ?? input + output,
  };
}

function isMistralFinishReason(value: string): boolean {
  return value === "stop" || value === "length" || value === "model_length" || value === "tool_calls";
}

function openAIChatStopReason(
  value: string | undefined,
  hasToolCalls: boolean,
): { stopReason: "stop" | "length" | "toolUse"; errorMessage?: string } {
  if (value === "stop" || value === "end") return { stopReason: hasToolCalls ? "toolUse" : "stop" };
  if (value === "length") return { stopReason: "length" };
  if (value === "function_call" || value === "tool_calls") return { stopReason: "toolUse" };
  if (value === undefined) return { stopReason: "stop", errorMessage: "OpenAI Chat stream ended without a finish reason" };
  return { stopReason: "stop", errorMessage: `Provider finish_reason: ${value}` };
}

function toOpenAIChatMessages(
  model: Model,
  context: Context,
  compat = openAICompletionsCompat(model),
): unknown[] {
  const result: unknown[] = [];
  if (context.systemPrompt) {
    result.push({
      role: model.reasoning && compat["supportsDeveloperRole"] === true ? "developer" : "system",
      content: context.systemPrompt,
    });
  }
  let previousRole: "user" | "assistant" | "tool" | undefined;
  for (const message of context.messages) {
    if (message.role === "user") {
      if (previousRole === "tool" && compat["requiresAssistantAfterToolResult"] === true) {
        result.push({ role: "assistant", content: "I have processed the tool results." });
      }
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
      previousRole = "user";
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
      const thinking = message.content.filter(
        (part): part is Extract<AssistantMessage["content"][number], { type: "thinking" }> =>
          part.type === "thinking" && part.thinking.length > 0,
      );
      const thinkingText = thinking.map((part) => part.thinking).join("\n\n");
      const reasoningField = thinking
        .map((part) => part.thinkingSignature)
        .find(isChatReasoningField);
      const reasoningDetails = message.content
        .filter((part): part is ToolCall => part.type === "toolCall")
        .map((call) => safeJson(call.thoughtSignature))
        .filter((value) => value !== undefined);
      const compat = compatRecord(model);
      const assistant = {
        role: "assistant",
        content:
          compat["requiresThinkingAsText"] === true && thinkingText !== ""
            ? `${thinkingText}${text === "" ? "" : `\n\n${text}`}`
            : text || (compat["requiresAssistantAfterToolResult"] === true ? "" : null),
        tool_calls: calls.length === 0 ? undefined : calls,
        reasoning_details: reasoningDetails.length === 0 ? undefined : reasoningDetails,
        ...(thinking.length === 0
          ? compat["requiresReasoningContentOnAssistantMessages"] === true
            ? { reasoning_content: "" }
            : undefined
          : reasoningField === undefined
            ? compat["requiresReasoningContentOnAssistantMessages"] === true
              ? { reasoning_content: thinking.map((part) => part.thinking).join("\n") }
              : undefined
            : { [reasoningField]: thinking.map((part) => part.thinking).join("\n") }),
      };
      result.push(assistant);
      previousRole = "assistant";
    } else {
      result.push({
        role: "tool",
        tool_call_id: message.toolCallId,
        name: compat["requiresToolResultName"] === true ? message.toolName : undefined,
        content: openAIChatToolResult(message.content),
      });
      previousRole = "tool";
    }
  }
  return result;
}

function isChatReasoningField(value: string | undefined): value is "reasoning_content" | "reasoning" | "reasoning_text" {
  return value === "reasoning_content" || value === "reasoning" || value === "reasoning_text";
}

function imageUrl(image: {
  readonly mimeType?: string;
  readonly data?: string;
  readonly url?: string;
}): string {
  if (image.url !== undefined) return image.url;
  return `data:${image.mimeType ?? "application/octet-stream"};base64,${image.data ?? ""}`;
}

function safeJson(value: string | undefined): unknown {
  if (value === undefined) return undefined;
  try {
    return JSON.parse(value);
  } catch {
    return value;
  }
}

function openAIChatToolResult(content: ToolResultMessage["content"]): unknown {
  const parts = content.map((part) =>
    part.type === "text"
      ? { type: "text", text: part.text }
      : { type: "image_url", image_url: { url: imageUrl(part) } },
  );
  return parts.some((part) => part.type === "image_url")
    ? parts
    : parts.map((part) => (part as { text: string }).text).join("\n");
}

function responsesToolResult(content: ToolResultMessage["content"]): unknown {
  const parts = content.map((part) =>
    part.type === "text"
      ? { type: "input_text", text: part.text }
      : { type: "input_image", image_url: imageUrl(part) },
  );
  return parts.some((part) => part.type === "input_image")
    ? parts
    : parts.map((part) => (part as { text: string }).text).join("\n");
}

function anthropicToolResult(content: ToolResultMessage["content"]): unknown {
  return content.map((part) =>
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
  );
}

function clampCacheKey(value: string | undefined): string | undefined {
  return value === undefined ? undefined : value.slice(0, 64);
}

function openAIResponseFormat(
  format: ModelsSimpleStreamOptions["responseFormat"],
): Record<string, unknown> | undefined {
  if (format === undefined || format.type === "text") return undefined;
  if (format.type === "json_object") return { type: "json_object" };
  return {
    type: "json_schema",
    json_schema: {
      name: format.jsonSchema?.name ?? "response",
      schema: format.jsonSchema?.schema ?? {},
      strict: format.jsonSchema?.strict,
    },
  };
}

function openAIResponsesTextFormat(
  format: ModelsSimpleStreamOptions["responseFormat"],
): Record<string, unknown> | undefined {
  if (format === undefined || format.type === "text") return undefined;
  if (format.type === "json_object") return { format: { type: "json_object" } };
  return {
    format: {
      type: "json_schema",
      name: format.jsonSchema?.name ?? "response",
      schema: format.jsonSchema?.schema ?? {},
      strict: format.jsonSchema?.strict,
    },
  };
}

function anthropicResponseFormat(
  format: ModelsSimpleStreamOptions["responseFormat"],
): Record<string, unknown> | undefined {
  if (format?.type !== "json_schema") return undefined;
  return { format: { type: "json_schema", schema: format.jsonSchema?.schema ?? {} } };
}

function anthropicThinkingKeep(keep: string | undefined): Record<string, unknown> | undefined {
  if (keep === undefined) return undefined;
  return { edits: [{ type: "clear_thinking_20251015", keep }] };
}

function resolveThinking(
  model: Model,
  requested: ModelsSimpleStreamOptions["reasoning"],
): string | number | undefined {
  if (!model.reasoning) return undefined;
  if (requested === "off") {
    const off = model.thinkingLevelMap?.["off"];
    // An explicit mapping is a wire value (some APIs use `none` rather than
    // omission).  Only `off: null` means the provider cannot disable
    // reasoning and must fall back to its normal default.
    if (off !== undefined && off !== null) return off;
    if (off === undefined) return undefined;
  }
  const configured =
    requested === "off" ? model.defaultThinkingLevel ?? "medium" : requested ?? model.defaultThinkingLevel;
  if (configured === undefined) return undefined;
  const map = model.thinkingLevelMap;
  if (map === undefined) return configured;
  const direct = map[configured];
  if (direct !== undefined) return direct ?? undefined;

  const levels = ["minimal", "low", "medium", "high", "xhigh", "max"];
  const requestedIndex = levels.indexOf(configured);
  if (requestedIndex < 0) return undefined;
  for (let index = requestedIndex; index < levels.length; index += 1) {
    const mapped = map[levels[index]!];
    if (mapped !== undefined && mapped !== null) return mapped;
  }
  for (let index = requestedIndex - 1; index >= 0; index -= 1) {
    const mapped = map[levels[index]!];
    if (mapped !== undefined && mapped !== null) return mapped;
  }
  return undefined;
}

function thinkingBudgetFromValue(value: string | number): number {
  return typeof value === "number" ? value : thinkingBudget(value.toLowerCase());
}

function compatRecord(model: Model): Readonly<Record<string, unknown>> {
  return model.compat ?? {};
}

/** Pi's OpenAI-compatible defaults, with models.json overrides as the final layer. */
function openAICompletionsCompat(model: Model): Readonly<Record<string, unknown>> {
  const target = `${model.provider} ${model.baseUrl}`.toLowerCase();
  const zai = /\bzai\b|api\.z\.ai|open\.bigmodel\.cn/u.test(target);
  const together = /\btogether\b|api\.together\.(?:ai|xyz)/u.test(target);
  const moonshot = /moonshot/u.test(target);
  const openRouter = /openrouter/u.test(target);
  const cloudflareWorkers = /cloudflare-workers-ai|api\.cloudflare\.com/u.test(target);
  const cloudflareGateway = /cloudflare-ai-gateway|gateway\.ai\.cloudflare\.com/u.test(target);
  const nvidia = /\bnvidia\b|integrate\.api\.nvidia\.com/u.test(target);
  const antLing = /ant-ling/u.test(target);
  const xai = /\bxai\b|api\.x\.ai/u.test(target);
  const deepseek = /deepseek/u.test(target);
  const nonStandard = zai || together || moonshot || cloudflareWorkers || cloudflareGateway || nvidia || antLing || xai || deepseek || /cerebras|chutes|opencode/u.test(target);
  const useMaxTokens = zai || together || moonshot || cloudflareGateway || nvidia || antLing || /chutes/u.test(target);
  const detected: Record<string, unknown> = {
    supportsStore: !nonStandard,
    supportsDeveloperRole: !nonStandard && !openRouter,
    supportsReasoningEffort: !(xai || zai || moonshot || together || cloudflareGateway || nvidia || antLing),
    supportsUsageInStreaming: true,
    maxTokensField: useMaxTokens ? "max_tokens" : "max_completion_tokens",
    requiresToolResultName: false,
    requiresAssistantAfterToolResult: false,
    requiresThinkingAsText: false,
    requiresReasoningContentOnAssistantMessages: deepseek,
    thinkingFormat: deepseek ? "deepseek" : zai ? "zai" : together ? "together" : antLing ? "ant-ling" : openRouter ? "openrouter" : "openai",
    openRouterRouting: {},
    vercelGatewayRouting: {},
    chatTemplateKwargs: {},
    zaiToolStream: false,
    supportsStrictMode: !(moonshot || together || cloudflareGateway || nvidia),
    supportsOpenAIGrammarTools: false,
    sendSessionAffinityHeaders: false,
    sessionAffinityFormat: openRouter ? "openrouter" : "openai",
  };
  return { ...detected, ...compatRecord(model) };
}

function openAIResponsesCompat(model: Model): Readonly<Record<string, unknown>> {
  const openRouter = /openrouter/u.test(`${model.provider} ${model.baseUrl}`.toLowerCase());
  return {
    sessionAffinityFormat: openRouter ? "openrouter" : "openai",
    sendSessionAffinityHeaders: false,
    ...compatRecord(model),
  };
}

function vercelGatewayOptions(compat: Readonly<Record<string, unknown>>): Record<string, unknown> | undefined {
  const routing = record(compat["vercelGatewayRouting"]);
  const only = stringArray(routing?.["only"]);
  const order = stringArray(routing?.["order"]);
  return only === undefined && order === undefined
    ? undefined
    : { gateway: { ...(only === undefined ? undefined : { only }), ...(order === undefined ? undefined : { order }) } };
}

function chatTemplateKwargs(
  compat: Readonly<Record<string, unknown>>,
  thinking: string | number | undefined,
): Record<string, string | number | boolean | null> | undefined {
  if (compat["thinkingFormat"] === "qwen-chat-template") {
    return { enable_thinking: thinking !== undefined, preserve_thinking: true };
  }
  if (compat["thinkingFormat"] !== "chat-template") return undefined;
  const configured = record(compat["chatTemplateKwargs"]);
  if (configured === undefined) return undefined;
  const result: Record<string, string | number | boolean | null> = {};
  for (const [name, value] of Object.entries(configured)) {
    const template = record(value);
    const variable = template?.["$var"];
    if (template?.["omitWhenOff"] === true && thinking === undefined) continue;
    const resolved = variable === "thinking.enabled"
      ? thinking !== undefined
      : variable === "thinking.effort"
        ? thinking
        : typeof value === "string" || typeof value === "number" || typeof value === "boolean" || value === null
          ? value
          : undefined;
    if (resolved !== undefined) result[name] = resolved;
  }
  return Object.keys(result).length === 0 ? undefined : result;
}

function stringArray(value: unknown): string[] | undefined {
  if (!Array.isArray(value) || !value.every((entry) => typeof entry === "string")) return undefined;
  return [...value];
}

function openAIChatReasoningFields(
  model: Model,
  compat: Readonly<Record<string, unknown>>,
  thinking: string | number | undefined,
): Record<string, unknown> {
  const format = compat["thinkingFormat"];
  const effort = supportsReasoningEffort(model, compat, thinking) ? thinking : undefined;
  if (format === "deepseek") {
    return {
      thinking: { type: thinking === undefined ? "disabled" : "enabled" },
      reasoning_effort: effort,
    };
  }
  if (format === "zai") {
    return {
      thinking: { type: thinking === undefined ? "disabled" : "enabled", clear_thinking: false },
      reasoning_effort: model.compat?.["supportsReasoningEffort"] === true ? thinking : undefined,
    };
  }
  if (format === "qwen") {
    return { enable_thinking: thinking !== undefined, reasoning_effort: effort };
  }
  if (format === "qwen-chat-template" || format === "chat-template") return {};
  if (format === "openrouter") return thinking === undefined ? {} : { reasoning: { effort: thinking } };
  if (format === "ant-ling") return thinking === undefined ? {} : { reasoning: { effort: thinking } };
  if (format === "together") {
    return { reasoning: { enabled: thinking !== undefined }, reasoning_effort: effort };
  }
  if (format === "string-thinking") {
    return thinking === undefined ? { thinking: "none" } : { thinking };
  }
  return { reasoning_effort: effort };
}

function responseReasoningEffort(
  model: Model,
  compat: Readonly<Record<string, unknown>>,
  thinking: string | number | undefined,
): string | number | undefined {
  return supportsReasoningEffort(model, compat, thinking) ? thinking : undefined;
}

function mistralReasoningEffort(
  model: Model,
  thinking: string | number | undefined,
): string | number | undefined {
  return supportsReasoningEffort(model, {}, thinking) ? thinking : undefined;
}

function supportsReasoningEffort(
  model: Model,
  compat: Readonly<Record<string, unknown>>,
  thinking: string | number | undefined,
): boolean {
  if (thinking === undefined) return false;
  if (compat["supportsReasoningEffort"] === true) return true;
  if (compat["supportsReasoningEffort"] === false) return false;
  if (model.thinkingLevelMap !== undefined) {
    return Object.values(model.thinkingLevelMap).some((value) => value === thinking);
  }
  if (model.api === "mistral-conversations") return false;
  const target = `${model.provider} ${model.baseUrl}`.toLowerCase();
  return !/xai|api\.x\.ai|moonshot|together|cloudflare|nvidia|ant-ling/iu.test(target);
}

function sessionAffinityHeaders(
  compat: Readonly<Record<string, unknown>>,
  sessionId: string | undefined,
): Record<string, string> | undefined {
  if (sessionId === undefined || compat["sendSessionAffinityHeaders"] !== true) return undefined;
  const format = compat["sessionAffinityFormat"];
  if (format === "openrouter") return { "x-session-id": sessionId };
  return {
    ...(format === "openai" ? { session_id: sessionId } : undefined),
    "x-client-request-id": sessionId,
    "x-session-affinity": sessionId,
  };
}

function responsesSessionAffinityHeaders(
  compat: Readonly<Record<string, unknown>>,
  sessionId: string | undefined,
): Record<string, string> | undefined {
  if (sessionId === undefined) return undefined;
  const format = compat["sessionAffinityFormat"];
  if (format === "openrouter") return { "x-session-id": sessionId };
  return {
    ...(format === "openai" ? { session_id: sessionId } : undefined),
    "x-client-request-id": sessionId,
  };
}

function codexSessionHeaders(sessionId: string | undefined): Record<string, string> | undefined {
  return sessionId === undefined
    ? undefined
    : { "session-id": sessionId, "x-client-request-id": sessionId };
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
        output: responsesToolResult(message.content),
      });
      continue;
    }
    const text = message.content
      .filter((part) => part.type === "text")
      .map((part) => part.text)
      .join("");
    if (isResponsesApi(message.api)) {
      for (const part of message.content) {
        if (part.type !== "thinking" || part.thinkingSignature === undefined) continue;
        result.push(
          part.reasoningItem === undefined
            ? {
                // Keep historical transcripts replayable. New transcript entries retain
                // the completed item below, including future fields unknown to us.
                type: "reasoning",
                id: part.itemId,
                encrypted_content: part.thinkingSignature,
                summary: [{ type: "summary_text", text: part.thinking }],
              }
            : { ...part.reasoningItem },
        );
      }
    }
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

function contextHasResponsesReasoning(context: Context): boolean {
  return context.messages.some(
    (message) =>
      message.role === "assistant" &&
      isResponsesApi(message.api) &&
      message.content.some(
        (part) => part.type === "thinking" && part.thinkingSignature !== undefined,
      ),
  );
}

function isResponsesApi(api: string): boolean {
  return api === "openai-responses" || api === "openai-codex-responses" || api === "azure-openai-responses";
}

function toAnthropicMessages(context: Context): unknown[] {
  const result: unknown[] = [];
  for (let index = 0; index < context.messages.length; index += 1) {
    const message = context.messages[index]!;
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
      const toolResults: ToolResultMessage[] = [message];
      while (context.messages[index + 1]?.role === "toolResult") {
        toolResults.push(context.messages[++index] as ToolResultMessage);
      }
      result.push({
        role: "user",
        content: toolResults.map((toolResult) => ({
          type: "tool_result",
          tool_use_id: toolResult.toolCallId,
          is_error: toolResult.isError,
          content: anthropicToolResult(toolResult.content),
        })),
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
  // Direct adapter callers have no `ProviderModels.getAuth(model)` pass, so
  // preserve static model headers (including host identity).  The resolved
  // auth layer is applied last: normal runtime calls carry the template-
  // resolved model layer there, retaining its final precedence and deletions.
  for (const source of [modelHeaders, auth.auth.headers]) {
    for (const [name, value] of Object.entries(source ?? {})) {
      if (value === null) headers.delete(name);
      else headers.set(name, value);
    }
  }
  if (
    auth.auth.apiKey !== undefined &&
    !headers.has("authorization") &&
    !headers.has("x-api-key") &&
    !headers.has("x-goog-api-key") &&
    !headers.has("api-key")
  ) {
    headers.set("authorization", `Bearer ${auth.auth.apiKey}`);
  }
  return headers;
}

/**
 * Copilot's model gateway distinguishes the origin of an agent continuation
 * and rejects image requests unless its VS Code integration headers are sent.
 * Keep this next to the common request-header merge so every compatible wire
 * protocol (Chat, Responses and Anthropic Messages) uses the same projection.
 */
function protocolRequestHeaders(
  model: Model,
  context: Context,
  auth: AuthResult,
  base: Record<string, string>,
): Headers {
  const headers = requestHeaders(model.headers, auth, base);
  if (model.provider !== "github-copilot") return headers;

  const hasImages = context.messages.some(
    (message) =>
      (message.role === "user" || message.role === "toolResult") &&
      Array.isArray(message.content) &&
      (message.content as readonly (TextContent | ImageContent)[]).some((part) => part.type === "image"),
  );
  const last = context.messages.at(-1);
  headers.set("editor-version", "vscode/1.107.0");
  headers.set("editor-plugin-version", "copilot-chat/0.35.0");
  headers.set("copilot-integration-id", "vscode-chat");
  headers.set("x-initiator", last === undefined || last.role === "user" ? "user" : "agent");
  headers.set("openai-intent", "conversation-edits");
  if (hasImages) headers.set("copilot-vision-request", "true");
  return headers;
}

async function assertOk(response: Response): Promise<void> {
  if (response.ok) return;
  const detail = await response.text().catch(() => "");
  const message = `Provider request failed (HTTP ${response.status}): ${detail || response.statusText}`;
  if (response.status === 413) throw new APIRequestTooLargeError(response.status, message);
  if (/context.{0,24}(length|window|token)|too many tokens/iu.test(message.toLowerCase())) {
    throw new APIContextOverflowError(response.status, message);
  }
  throw new APIStatusError(response.status, message);
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
  return { type: "error", reason: aborted ? "aborted" : "error", error: output, cause: error };
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

function appendThinking(
  output: AssistantMessage,
  delta: string,
): Extract<AssistantMessage["content"][number], { type: "thinking" }> {
  const current = output.content.at(-1);
  if (current?.type === "thinking") {
    current.thinking += delta;
    return current;
  }
  const created = { type: "thinking" as const, thinking: delta };
  output.content.push(created);
  return created;
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

/** Azure's v1 base is special; preserve proxy query parameters but normalize Azure hosts. */
function azureResponsesEndpoint(baseUrl: string): string {
  const url = new URL(baseUrl);
  const azureHost =
    url.hostname.endsWith(".openai.azure.com") ||
    url.hostname.endsWith(".cognitiveservices.azure.com") ||
    url.hostname.endsWith(".ai.azure.com");
  const path = url.pathname.replace(/\/+$/u, "");
  if (
    azureHost &&
    (path === "" || path === "/" || path === "/openai" || path === "/openai/v1" || path === "/openai/v1/responses")
  ) {
    url.pathname = "/openai/v1/responses";
    url.search = "";
  } else {
    url.pathname = `${path}/responses`.replace(/^\/\/+/u, "/");
  }
  return url.toString();
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
