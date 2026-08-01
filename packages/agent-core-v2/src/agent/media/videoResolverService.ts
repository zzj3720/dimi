import { LifecycleScope, ScopeActivation, registerScopedService } from "#/_base/di/scope";
import type { ContentPart, Message } from "#/llmProtocol/message";
import type { ModelRequester } from "#/app/modelCatalog/modelRequester";

import { type DimiFileRef, isDimiFileUrl, parseDimiFileUrl } from "./dimiFileUrl";
import { IAgentVideoResolverService } from "./videoResolver";

const VIDEO_UNAVAILABLE_TEXT = "[video omitted: the uploaded file is no longer available]";

/**
 * The provider contract accepts text and images. Legacy Dimi video handles
 * are therefore projected to a textual file reference before model dispatch.
 */
export class AgentVideoResolverService implements IAgentVideoResolverService {
  declare readonly _serviceBrand: undefined;

  async resolve(
    messages: readonly Message[],
    _requester: ModelRequester,
    _signal?: AbortSignal,
  ): Promise<readonly Message[]> {
    return messages.map((message) => {
      const content = message.content.map((part) => {
        if (part.type !== "video_url" || !isDimiFileUrl(part.videoUrl.url)) return part;
        const ref = parseDimiFileUrl(part.videoUrl.url);
        return ref === undefined ? part : tag(ref);
      });
      return { ...message, content };
    });
  }
}

function tag(ref: DimiFileRef): ContentPart {
  if (ref.path === undefined || ref.path.length === 0) {
    return { type: "text", text: VIDEO_UNAVAILABLE_TEXT };
  }
  return {
    type: "text",
    text: `<video path="${escapeAttribute(ref.path)}"></video>`,
  };
}

function escapeAttribute(value: string): string {
  return value
    .replaceAll("&", "&amp;")
    .replaceAll('"', "&quot;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;");
}

registerScopedService(
  LifecycleScope.Agent,
  IAgentVideoResolverService,
  AgentVideoResolverService,
  ScopeActivation.OnScopeCreated,
  "media",
);
