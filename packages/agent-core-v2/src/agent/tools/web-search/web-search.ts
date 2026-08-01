/**
 * `tools` domain (L7) — `IWebSearchTool` contract (the `WebSearch` tool).
 *
 * Public contract of the `WebSearch` builtin tool: the model-facing
 * `WebSearchInputSchema` / `WebSearchInput` and the `IWebSearchTool` DI
 * decorator that the implementation (`webSearchTool.ts`) registers against
 * via `registerAgentToolService`. The backend contract lives in the App-scope
 * `webSearch` domain. Bound at Agent scope.
 */

import { z } from "zod";

import { createDecorator } from "#/_base/di/instantiation";
import { type AgentTool } from "#/tool/toolContract";

export const WebSearchInputSchema = z.object({
  query: z.string().describe("The query text to search for."),
});

export type WebSearchInput = z.infer<typeof WebSearchInputSchema>;

export interface IWebSearchTool extends AgentTool<WebSearchInput> {
  readonly _serviceBrand: undefined;
}
export const IWebSearchTool = createDecorator<IWebSearchTool>("webSearchTool");
