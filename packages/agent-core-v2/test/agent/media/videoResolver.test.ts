import { describe, expect, it } from "vitest";

import { buildKimiFileUrl, parseKimiFileUrl } from "#/agent/media/kimiFileUrl";
import { AgentVideoResolverService } from "#/agent/media/videoResolverService";
import type { Message } from "#/llmProtocol/message";
import type { ModelRequester } from "#/app/modelCatalog/modelRequester";

const requester = {} as ModelRequester;

function videoMessage(url: string): Message {
  return {
    role: "user",
    content: [{ type: "video_url", videoUrl: { url } }],
    toolCalls: [],
  };
}

describe("kimiFileUrl", () => {
  it("round-trips a file id and an escaped materialization path", () => {
    const url = buildKimiFileUrl("file_1", "/a b/clip.mp4");
    expect(url).toBe(`kimi-file://file_1?path=${encodeURIComponent("/a b/clip.mp4")}`);
    expect(parseKimiFileUrl(url)).toEqual({
      fileId: "file_1",
      path: "/a b/clip.mp4",
    });
  });

  it("returns undefined for non-kimi-file URLs", () => {
    expect(parseKimiFileUrl("https://example.com/clip.mp4")).toBeUndefined();
  });
});

describe("AgentVideoResolverService", () => {
  it("projects a legacy uploaded-video reference to a textual file reference", async () => {
    const resolver = new AgentVideoResolverService();
    const [message] = await resolver.resolve(
      [videoMessage(buildKimiFileUrl("file_1", "/tmp/clip.mp4"))],
      requester,
    );

    expect(message?.content).toEqual([
      { type: "text", text: '<video path="/tmp/clip.mp4"></video>' },
    ]);
  });

  it("uses an unavailable placeholder when the file path is absent", async () => {
    const resolver = new AgentVideoResolverService();
    const [message] = await resolver.resolve([videoMessage(buildKimiFileUrl("file_1"))], requester);

    expect(message?.content).toEqual([
      {
        type: "text",
        text: "[video omitted: the uploaded file is no longer available]",
      },
    ]);
  });

  it("keeps ordinary messages unchanged", async () => {
    const resolver = new AgentVideoResolverService();
    const messages = [videoMessage("https://example.com/clip.mp4")];

    expect(await resolver.resolve(messages, requester)).toEqual(messages);
  });
});
