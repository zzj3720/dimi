import { randomBytes } from "node:crypto";
import { mkdtemp, rm } from "node:fs/promises";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { join } from "node:path";

import {
  createDeviceIdentity,
  parsePairingUri,
  RemoteClient,
  type RelayDataFrame,
  type SocketLike,
} from "@k-3720/remote";
import { startRemoteBridge } from "@k-3720/remote/bridge";
import { afterEach, describe, expect, it } from "vitest";
import { WebSocket, WebSocketServer, type RawData } from "ws";

import { startRelay } from "./server";

const cleanups: (() => Promise<void>)[] = [];

afterEach(async () => {
  for (const cleanup of cleanups.splice(0).toReversed()) await cleanup();
});

describe("remote mobile path", () => {
  it("pairs, forwards only encrypted allowed traffic, and recovers after bridge restart", async () => {
    let mutationCount = 0;
    const localWsMessages: string[] = [];
    const localHttp = createServer((request, response) => {
      let body = "";
      request.on("data", (chunk) => {
        body += chunk.toString();
      });
      request.on("end", () => {
        if (request.method === "POST") mutationCount += 1;
        response.writeHead(200, { "Content-Type": "application/json" });
        response.end(
          JSON.stringify({
            code: 0,
            msg: "success",
            request_id: "req_test",
            data:
              request.method === "POST"
                ? { path: request.url, received: body.length > 0 ? JSON.parse(body) : null }
                : { items: [], has_more: false },
          }),
        );
      });
    });
    const localWs = new WebSocketServer({
      server: localHttp,
      handleProtocols: (protocols) => protocols.values().next().value ?? false,
    });
    localWs.on("connection", (socket) => {
      socket.on("message", (data) => {
        const message = rawDataToString(data);
        localWsMessages.push(message);
        socket.send(`runtime:${message}`);
      });
    });
    await listen(localHttp);
    cleanups.push(
      () =>
        new Promise<void>((resolve) => {
          localWs.close(() => {
            localHttp.close(() => {
              resolve();
            });
          });
        }),
    );
    const localAddress = localHttp.address();
    const localPort =
      typeof localAddress === "object" && localAddress !== null ? localAddress.port : 0;

    let relayUrl = process.env["K3720_RELAY_E2E_URL"];
    if (relayUrl === undefined) {
      const relay = await startRelay();
      cleanups.push(() => relay.close());
      relayUrl = `ws://${relay.host}:${relay.port}`;
    }
    const stateDir = await mkdtemp(join(tmpdir(), "remote-e2e-"));
    cleanups.push(() => rm(stateDir, { recursive: true, force: true }));
    const bridgeOptions = {
      relayUrl,
      localOrigin: `http://127.0.0.1:${localPort}`,
      localToken: "local-test-token",
      runtimeName: "Test runtime",
      statePath: join(stateDir, "bridge.json"),
    };
    const firstBridge = await startRemoteBridge(bridgeOptions);
    cleanups.push(() => firstBridge.close());

    const pairingUri = firstBridge.createPairingUri();
    expect(pairingUri).toMatch(/^k-3720:\/\/pair\?/);
    const pairing = parsePairingUri(pairingUri);
    const identity = createDeviceIdentity("Test phone", (length) => randomBytes(length));
    let paired = false;
    let socket: WebSocket | undefined;
    const outbound: string[] = [];
    const states: string[] = [];
    const client = new RemoteClient({
      pairing,
      identity,
      randomBytes: (length) => randomBytes(length),
      reconnectDelayMs: () => 20,
      createSocket: (url) => {
        socket = new WebSocket(url);
        const originalSend = socket.send.bind(socket);
        socket.send = ((data: string) => {
          outbound.push(data);
          originalSend(data);
        }) as WebSocket["send"];
        return socket as unknown as SocketLike;
      },
      onPaired: () => {
        paired = true;
      },
    });
    cleanups.push(async () => {
      client.close();
    });
    client.onState((state) => states.push(state));
    client.start();
    await eventually(() => client.state === "online");
    expect(paired).toBe(true);

    const postStartedAt = outbound.length;
    const prompt = await client.request("POST", "/api/v1/sessions/s1/prompts", {
      content: [{ type: "text", text: "hello" }],
    });
    expect(prompt.body).toMatchObject({
      code: 0,
      data: { path: "/api/v1/sessions/s1/prompts" },
    });
    expect(mutationCount).toBe(1);
    const encryptedRequest = outbound
      .slice(postStartedAt)
      .map((value) => JSON.parse(value) as unknown)
      .find(
        (value): value is RelayDataFrame =>
          typeof value === "object" &&
          value !== null &&
          (value as { type?: unknown }).type === "data",
      );
    expect(encryptedRequest).toBeDefined();
    expect(JSON.stringify(encryptedRequest)).not.toContain("/api/v1/sessions");
    socket?.send(JSON.stringify(encryptedRequest));
    await new Promise((resolve) => setTimeout(resolve, 40));
    expect(mutationCount).toBe(1);

    await expect(client.request("GET", "/api/v1/terminals")).rejects.toThrow(
      "not available remotely",
    );
    await expect(client.request("GET", "/api/v1/meta")).rejects.toThrow(
      "not available remotely",
    );
    await expect(client.request("GET", "/api/v1/sessions/s1/status")).rejects.toThrow(
      "not available remotely",
    );
    await expect(client.request("GET", "/api/v1/sessions/s1/transcript/ops")).rejects.toThrow(
      "not available remotely",
    );

    const wsMessages: string[] = [];
    client.onWsMessage((data) => wsMessages.push(data));
    client.sendWs(JSON.stringify({ type: "client_hello", id: "hello", payload: {} }));
    await new Promise((resolve) => setTimeout(resolve, 40));
    expect(localWsMessages).toEqual([]);

    client.sendWs(
      JSON.stringify({
        type: "client_hello",
        id: "hello",
        payload: { client_id: "phone" },
      }),
    );
    client.sendWs(
      JSON.stringify({
        type: "subscribe",
        id: "sub",
        payload: { session_ids: ["s1"] },
      }),
    );
    client.sendWs(
      JSON.stringify({
        type: "subscribe_v2",
        id: "transcript",
        payload: { session_id: "s1", transcript: { main: "block" } },
      }),
    );
    await eventually(
      () =>
        localWsMessages.length === 3 &&
        wsMessages.filter((message) => message.startsWith("runtime:")).length === 3,
    );
    expect(wsMessages.filter((message) => message.startsWith("runtime:"))).toHaveLength(3);

    const allowedCount = localWsMessages.length;
    client.sendWs(
      JSON.stringify({
        type: "watch_fs_add",
        id: "watch",
        payload: { session_id: "s1", paths: ["/workspace"], recursive: true },
      }),
    );
    client.sendWs(
      JSON.stringify({
        type: "subscribe",
        id: "smuggled-watch",
        payload: {
          session_ids: ["s1"],
          watch_fs: { s1: { paths: ["/workspace"], recursive: true } },
        },
      }),
    );
    client.sendWs(
      JSON.stringify({
        type: "terminal_input",
        id: "terminal",
        payload: { session_id: "s1", terminal_id: "t1", data: "whoami\n" },
      }),
    );
    client.sendWs(
      JSON.stringify({
        type: "subscribe_v2",
        id: "delta",
        payload: { session_id: "s1", transcript: { main: "delta" } },
      }),
    );
    await new Promise((resolve) => setTimeout(resolve, 60));
    expect(localWsMessages).toHaveLength(allowedCount);

    await firstBridge.close();
    await eventually(() => client.state === "disconnected");
    const secondBridge = await startRemoteBridge(bridgeOptions);
    cleanups.push(() => secondBridge.close());
    await eventually(() => client.state === "online");
    expect(states.filter((state) => state === "online")).toHaveLength(2);

    const abort = await client.request("POST", "/api/v1/sessions/s1:abort", {});
    expect(abort.body).toMatchObject({ code: 0, data: { path: "/api/v1/sessions/s1:abort" } });
    expect(mutationCount).toBe(2);
  });
});

function listen(server: ReturnType<typeof createServer>): Promise<void> {
  return new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => {
      server.off("error", reject);
      resolve();
    });
  });
}

async function eventually(predicate: () => boolean, timeoutMs = 5_000): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  while (!predicate()) {
    if (Date.now() >= deadline) throw new Error("Timed out waiting for condition.");
    await new Promise((resolve) => setTimeout(resolve, 10));
  }
}

function rawDataToString(data: RawData): string {
  if (Array.isArray(data)) return Buffer.concat(data).toString("utf8");
  if (data instanceof ArrayBuffer) return Buffer.from(new Uint8Array(data)).toString("utf8");
  return data.toString("utf8");
}
