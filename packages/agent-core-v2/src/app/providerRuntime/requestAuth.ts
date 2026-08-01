/**
 * `providerRuntime` domain (L2) — cloud request authentication projection.
 *
 * Converts already-selected Bedrock default-chain and Vertex ADC credentials
 * into protocol request headers. It is deliberately called immediately before
 * fetch so AWS signatures and Google access tokens are fresh per request.
 */
import { Sha256 } from "@aws-crypto/sha256-js";
import { defaultProvider } from "@aws-sdk/credential-provider-node";
import { HttpRequest } from "@smithy/protocol-http";
import { SignatureV4 } from "@smithy/signature-v4";
import { GoogleAuth } from "google-auth-library";

import type { AuthResult, Model } from "./types";

export interface AuthenticatedRequest {
  readonly url: string;
  readonly headers: Headers;
}

/** Applies only provider-native cloud auth; ordinary API keys stay in stream.ts. */
export async function authenticateProviderRequest(
  model: Model,
  auth: AuthResult,
  url: string,
  headers: Headers,
  body: string,
): Promise<AuthenticatedRequest> {
  if (model.api === "google-vertex" && auth.auth.apiKey === undefined) {
    const resolvedUrl = vertexUrl(url, auth.env);
    const token = await vertexAccessToken(auth.env);
    if (!headers.has("authorization")) headers.set("authorization", `Bearer ${token}`);
    return { url: resolvedUrl, headers };
  }
  if (model.api === "bedrock-converse-stream" && auth.auth.apiKey === undefined && !headers.has("authorization")) {
    const resolvedUrl = new URL(url);
    const request = new HttpRequest({
      protocol: resolvedUrl.protocol,
      hostname: resolvedUrl.hostname,
      port: resolvedUrl.port === "" ? undefined : Number(resolvedUrl.port),
      method: "POST",
      path: `${resolvedUrl.pathname}${resolvedUrl.search}`,
      headers: Object.fromEntries(headers.entries()),
      body,
    });
    const signed = await new SignatureV4({
      credentials: defaultProvider({ profile: auth.env?.["AWS_PROFILE"] }),
      region: bedrockRegion(model, resolvedUrl, auth.env),
      service: "bedrock",
      sha256: Sha256,
    }).sign(request);
    return { url, headers: new Headers(signed.headers) };
  }
  return { url, headers };
}

function vertexUrl(value: string, env: Readonly<Record<string, string>> | undefined): string {
  const project = env?.["GOOGLE_CLOUD_PROJECT"];
  const location = env?.["GOOGLE_CLOUD_LOCATION"];
  if (!project || !location) throw new Error("Google Vertex ADC requires GOOGLE_CLOUD_PROJECT and GOOGLE_CLOUD_LOCATION");
  const url = new URL(value.replaceAll("{location}", location).replaceAll("%7Blocation%7D", location));
  const prefix = `/v1/projects/${encodeURIComponent(project)}/locations/${encodeURIComponent(location)}`;
  if (!url.pathname.startsWith("/v1/projects/")) url.pathname = `${prefix}${url.pathname}`;
  return url.toString();
}

async function vertexAccessToken(env: Readonly<Record<string, string>> | undefined): Promise<string> {
  const client = new GoogleAuth({
    projectId: env?.["GOOGLE_CLOUD_PROJECT"],
    keyFilename: env?.["GOOGLE_APPLICATION_CREDENTIALS"],
    scopes: ["https://www.googleapis.com/auth/cloud-platform"],
  });
  const token = await client.getAccessToken();
  if (!token) throw new Error("Google Application Default Credentials produced no access token");
  return token;
}

function bedrockRegion(
  model: Model,
  url: URL,
  env: Readonly<Record<string, string>> | undefined,
): string {
  const arnRegion = /^arn:aws(?:-[a-z0-9-]+)?:bedrock:([a-z0-9-]+):/u.exec(model.id)?.[1];
  if (arnRegion) return arnRegion;
  const endpointRegion = /^bedrock-runtime\.([a-z0-9-]+)\.amazonaws\.com$/u.exec(url.hostname)?.[1];
  return env?.["AWS_REGION"] ?? env?.["AWS_DEFAULT_REGION"] ?? endpointRegion ?? "us-east-1";
}
