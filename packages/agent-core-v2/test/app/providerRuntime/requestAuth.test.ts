import { afterEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  defaultProvider: vi.fn(),
  sign: vi.fn(),
  getAccessToken: vi.fn(),
}));

vi.mock("@aws-sdk/credential-provider-node", () => ({
  defaultProvider: mocks.defaultProvider,
}));
vi.mock("@smithy/signature-v4", () => ({
  SignatureV4: class {
    sign = mocks.sign;
  },
}));
vi.mock("google-auth-library", () => ({
  GoogleAuth: class {
    getAccessToken = mocks.getAccessToken;
  },
}));

import { authenticateProviderRequest } from "#/app/providerRuntime/requestAuth";
import type { AuthResult, Model } from "#/app/providerRuntime/types";

const base = {
  name: "Example",
  provider: "example",
  reasoning: false,
  input: ["text"] as const,
  cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
  contextWindow: 1,
  maxTokens: 1,
};

afterEach(() => {
  vi.clearAllMocks();
});

describe("cloud request authentication projection", () => {
  it("signs an ambient Bedrock request immediately before dispatch", async () => {
    mocks.defaultProvider.mockReturnValue(async () => ({
      accessKeyId: "example-access-key",
      secretAccessKey: "example-secret",
    }));
    mocks.sign.mockImplementation(async (request: { headers: Record<string, string> }) => ({
      headers: {
        ...request.headers,
        authorization: "AWS4-HMAC-SHA256 Credential=example-access-key/...",
        "x-amz-date": "20260731T000000Z",
      },
    }));
    const model: Model = {
      ...base,
      id: "anthropic.example",
      api: "bedrock-converse-stream",
      baseUrl: "https://bedrock-runtime.us-east-1.amazonaws.com",
    };
    const auth: AuthResult = { auth: {}, env: { AWS_PROFILE: "example" } };

    const request = await authenticateProviderRequest(
      model,
      auth,
      "https://bedrock-runtime.us-east-1.amazonaws.com/model/anthropic.example/converse-stream",
      new Headers({ "content-type": "application/json" }),
      '{"messages":[]}',
    );

    expect(mocks.defaultProvider).toHaveBeenCalledWith({ profile: "example" });
    expect(mocks.sign).toHaveBeenCalledOnce();
    expect(request.url).toBe("https://bedrock-runtime.us-east-1.amazonaws.com/model/anthropic.example/converse-stream");
    expect(request.headers.get("authorization")).toBe("AWS4-HMAC-SHA256 Credential=example-access-key/...");
    expect(request.headers.get("x-amz-date")).toBe("20260731T000000Z");
  });

  it("uses ADC and synthesizes the project/location Vertex target", async () => {
    mocks.getAccessToken.mockResolvedValue("adc-access-token");
    const model: Model = {
      ...base,
      id: "gemini-example",
      api: "google-vertex",
      baseUrl: "https://{location}-aiplatform.googleapis.com",
    };
    const auth: AuthResult = {
      auth: {},
      env: {
        GOOGLE_CLOUD_PROJECT: "example-project",
        GOOGLE_CLOUD_LOCATION: "us-central1",
      },
    };

    const request = await authenticateProviderRequest(
      model,
      auth,
      "https://{location}-aiplatform.googleapis.com/publishers/google/models/gemini-example:streamGenerateContent?alt=sse",
      new Headers({ accept: "text/event-stream" }),
      "{}",
    );

    expect(mocks.getAccessToken).toHaveBeenCalledOnce();
    expect(request.url).toBe(
      "https://us-central1-aiplatform.googleapis.com/v1/projects/example-project/locations/us-central1/publishers/google/models/gemini-example:streamGenerateContent?alt=sse",
    );
    expect(request.headers.get("authorization")).toBe("Bearer adc-access-token");
  });
});
