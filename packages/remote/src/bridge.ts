import { randomBytes as nodeRandomBytes } from "node:crypto";
import { chmod, mkdir, readFile, rename, writeFile } from "node:fs/promises";
import { dirname } from "node:path";

import { WebSocket, type RawData } from "ws";
import { z } from "zod";

import {
  buildRelaySocketUrl,
  buildPairingUri,
  relayFrameSchema,
  type HttpRequestPacket,
  type PairingDescriptor,
  type RelayDataFrame,
  type RemotePacket,
} from "./contract";
import {
  createRuntimeIdentity,
  openPacket,
  randomPacketId,
  randomToken,
  sealPacket,
  type RandomBytes,
} from "./crypto";

const PAIRING_LIFETIME_MS = 10 * 60_000;
const RECONNECT_DELAY_MS = 2_000;
const WS_BEARER_PROTOCOL_PREFIX = "kimi-code.bearer.";
const MAX_COMPLETED_REQUESTS = 128;
const remoteWsFrameSchema = z.discriminatedUnion("type", [
  z
    .object({
      type: z.literal("client_hello"),
      id: z.string().min(1),
      payload: z.object({ client_id: z.string().min(1) }).strict(),
    })
    .strict(),
  z
    .object({
      type: z.literal("subscribe"),
      id: z.string().min(1),
      payload: z.object({ session_ids: z.array(z.string().min(1)) }).strict(),
    })
    .strict(),
  z
    .object({
      type: z.literal("unsubscribe"),
      id: z.string().min(1),
      payload: z.object({ session_ids: z.array(z.string().min(1)) }).strict(),
    })
    .strict(),
  z
    .object({
      type: z.literal("subscribe_v2"),
      id: z.string().min(1),
      payload: z
        .object({
          session_id: z.string().min(1),
          transcript: z.record(z.string(), z.enum(["off", "turn", "block"])),
          transcript_since: z.record(z.string(), z.number().int().nonnegative()).optional(),
        })
        .strict(),
    })
    .strict(),
  z
    .object({
      type: z.literal("unsubscribe_v2"),
      id: z.string().min(1),
      payload: z
        .object({
          session_id: z.string().min(1),
          agent_ids: z.array(z.string().min(1)).min(1).optional(),
        })
        .strict(),
    })
    .strict(),
]);

interface StoredDevice {
  deviceId: string;
  deviceName: string;
  publicKey: string;
  completed: {
    requestId: string;
    response: Extract<RemotePacket, { type: "http.response" }>;
  }[];
}

interface BridgeState {
  version: 1;
  runtimeId: string;
  publicKey: string;
  secretKey: string;
  devices: StoredDevice[];
}

export interface RemoteBridgeOptions {
  readonly relayUrl: string;
  readonly localOrigin: string;
  readonly localToken: string;
  readonly runtimeName: string;
  readonly statePath: string;
  readonly randomBytes?: RandomBytes;
  readonly now?: () => number;
  readonly onStatus?: (status: "connecting" | "online" | "offline") => void;
}

export interface RunningRemoteBridge {
  readonly runtimeId: string;
  readonly pairing: PairingDescriptor;
  readonly pairingUri: string;
  close(): Promise<void>;
}

export async function startRemoteBridge(
  options: RemoteBridgeOptions,
): Promise<RunningRemoteBridge> {
  const bridge = await RemoteBridge.create(options);
  bridge.start();
  return {
    runtimeId: bridge.pairing.runtimeId,
    pairing: bridge.pairing,
    pairingUri: buildPairingUri(bridge.pairing),
    close: () => bridge.close(),
  };
}

class RemoteBridge {
  readonly pairing: PairingDescriptor;
  readonly #options: RemoteBridgeOptions;
  readonly #randomBytes: RandomBytes;
  readonly #now: () => number;
  readonly #state: BridgeState;
  readonly #pairingToken: string;
  readonly #localSockets = new Map<string, WebSocket>();
  readonly #seenPacketIds = new Map<string, Set<string>>();

  #relay?: WebSocket;
  #desired = false;
  #reconnectTimer?: ReturnType<typeof setTimeout>;
  #saveQueue: Promise<void> = Promise.resolve();
  #pairingConsumed = false;

  private constructor(
    options: RemoteBridgeOptions,
    state: BridgeState,
    randomBytes: RandomBytes,
    now: () => number,
  ) {
    this.#options = options;
    this.#state = state;
    this.#randomBytes = randomBytes;
    this.#now = now;
    this.#pairingToken = randomToken(randomBytes);
    this.pairing = {
      relayUrl: options.relayUrl,
      runtimeId: state.runtimeId,
      runtimeName: options.runtimeName,
      runtimePublicKey: state.publicKey,
      token: this.#pairingToken,
      expiresAt: now() + PAIRING_LIFETIME_MS,
    };
  }

  static async create(options: RemoteBridgeOptions): Promise<RemoteBridge> {
    const randomBytes = options.randomBytes ?? ((length) => nodeRandomBytes(length));
    const now = options.now ?? Date.now;
    const state = await loadState(options.statePath, randomBytes);
    return new RemoteBridge(options, state, randomBytes, now);
  }

  start(): void {
    if (this.#desired) return;
    this.#desired = true;
    this.#connect();
  }

  async close(): Promise<void> {
    this.#desired = false;
    if (this.#reconnectTimer !== undefined) clearTimeout(this.#reconnectTimer);
    this.#reconnectTimer = undefined;
    for (const socket of this.#localSockets.values()) socket.close();
    this.#localSockets.clear();
    this.#relay?.close();
    this.#relay = undefined;
    await this.#saveQueue;
    this.#options.onStatus?.("offline");
  }

  #connect(): void {
    if (!this.#desired) return;
    this.#options.onStatus?.("connecting");
    const registration = {
      type: "register",
      protocol: 1,
      role: "bridge",
      runtimeId: this.#state.runtimeId,
    } as const;
    const socket = new WebSocket(buildRelaySocketUrl(this.#options.relayUrl, registration));
    this.#relay = socket;
    socket.on("open", () => {
      socket.send(JSON.stringify(registration));
    });
    socket.on("message", (data) => {
      this.#onRelayMessage(rawDataToString(data));
    });
    socket.on("error", () => {
      socket.close();
    });
    socket.on("close", () => {
      this.#onRelayClose(socket);
    });
  }

  #onRelayMessage(value: string): void {
    let parsed: unknown;
    try {
      parsed = JSON.parse(value);
    } catch {
      return;
    }
    const result = relayFrameSchema.safeParse(parsed);
    if (!result.success) return;
    const frame = result.data;
    if (frame.type === "registered") {
      this.#options.onStatus?.("online");
      return;
    }
    if (frame.type === "presence") {
      if (!frame.connected) this.#closeLocalSocket(frame.deviceId);
      return;
    }
    if (frame.type === "data") void this.#onData(frame);
  }

  async #onData(frame: RelayDataFrame): Promise<void> {
    let packet: RemotePacket;
    try {
      packet = openPacket(frame.payload, frame.publicKey, this.#state.secretKey);
    } catch {
      return;
    }

    const device = this.#state.devices.find((entry) => entry.deviceId === frame.deviceId);
    if (device === undefined) {
      await this.#pair(frame, packet);
      return;
    }
    if (device.publicKey !== frame.publicKey || this.#wasSeen(device.deviceId, packet.id)) return;
    this.#rememberPacket(device.deviceId, packet.id);

    switch (packet.type) {
      case "http.request":
        await this.#forwardHttp(device, packet);
        return;
      case "ws.open":
        this.#openLocalSocket(device, packet.id);
        return;
      case "ws.send":
        if (isAllowedRemoteWsFrame(packet.data)) {
          this.#localSockets.get(device.deviceId)?.send(packet.data);
        }
        return;
      case "ws.close":
        this.#closeLocalSocket(device.deviceId);
        return;
      case "http.response":
      case "pair.accepted":
      case "pair.request":
      case "remote.error":
      case "ws.message":
      case "ws.opened":
        return;
    }
  }

  async #pair(frame: RelayDataFrame, packet: RemotePacket): Promise<void> {
    if (
      packet.type !== "pair.request" ||
      this.#pairingConsumed ||
      packet.token !== this.#pairingToken ||
      this.#now() > this.pairing.expiresAt
    ) {
      return;
    }
    this.#pairingConsumed = true;
    const device: StoredDevice = {
      deviceId: frame.deviceId,
      deviceName: packet.deviceName,
      publicKey: frame.publicKey,
      completed: [],
    };
    this.#state.devices.push(device);
    await this.#save();
    this.#send(device, {
      type: "pair.accepted",
      id: randomPacketId(this.#randomBytes),
      sentAt: this.#now(),
      requestId: packet.id,
      runtimeName: this.#options.runtimeName,
    });
  }

  async #forwardHttp(device: StoredDevice, packet: HttpRequestPacket): Promise<void> {
    const cached = device.completed.find((entry) => entry.requestId === packet.id);
    if (cached !== undefined) {
      this.#send(device, cached.response);
      return;
    }
    if (!isAllowedRemoteRequest(packet.method, packet.path)) {
      this.#sendError(
        device,
        packet.id,
        "route_not_allowed",
        "This route is not available remotely.",
      );
      return;
    }
    try {
      const headers: Record<string, string> = {
        Authorization: `Bearer ${this.#options.localToken}`,
      };
      const init: RequestInit = {
        method: packet.method,
        headers,
      };
      if (packet.body !== undefined) {
        headers["Content-Type"] = "application/json";
        init.body = JSON.stringify(packet.body);
      }
      const response = await fetch(new URL(packet.path, this.#options.localOrigin), init);
      const contentType = response.headers.get("content-type") ?? "";
      const body = contentType.includes("application/json")
        ? await response.json()
        : await response.text();
      const responsePacket: Extract<RemotePacket, { type: "http.response" }> = {
        type: "http.response",
        id: randomPacketId(this.#randomBytes),
        sentAt: this.#now(),
        requestId: packet.id,
        status: response.status,
        body,
      };
      if (packet.method === "POST") {
        device.completed.push({ requestId: packet.id, response: responsePacket });
        device.completed.splice(0, Math.max(0, device.completed.length - MAX_COMPLETED_REQUESTS));
        await this.#save();
      }
      this.#send(device, responsePacket);
    } catch (error) {
      this.#sendError(
        device,
        packet.id,
        "local_runtime_unavailable",
        error instanceof Error ? error.message : String(error),
      );
    }
  }

  #openLocalSocket(device: StoredDevice, requestId: string): void {
    this.#closeLocalSocket(device.deviceId);
    const url = new URL("/api/v1/ws", this.#options.localOrigin);
    url.protocol = url.protocol === "https:" ? "wss:" : "ws:";
    const socket = new WebSocket(url, [`${WS_BEARER_PROTOCOL_PREFIX}${this.#options.localToken}`]);
    this.#localSockets.set(device.deviceId, socket);
    socket.on("open", () => {
      this.#send(device, {
        type: "ws.opened",
        id: randomPacketId(this.#randomBytes),
        sentAt: this.#now(),
        requestId,
      });
    });
    socket.on("message", (data) => {
      this.#send(device, {
        type: "ws.message",
        id: randomPacketId(this.#randomBytes),
        sentAt: this.#now(),
        data: rawDataToString(data),
      });
    });
    socket.on("error", () => {
      socket.close();
    });
    socket.on("close", () => {
      if (this.#localSockets.get(device.deviceId) === socket) {
        this.#localSockets.delete(device.deviceId);
      }
    });
  }

  #closeLocalSocket(deviceId: string): void {
    this.#localSockets.get(deviceId)?.close();
    this.#localSockets.delete(deviceId);
  }

  #send(device: StoredDevice, packet: RemotePacket): void {
    if (this.#relay?.readyState !== WebSocket.OPEN) return;
    this.#relay.send(
      JSON.stringify({
        type: "data",
        deviceId: device.deviceId,
        publicKey: this.#state.publicKey,
        payload: sealPacket(packet, device.publicKey, this.#state.secretKey, this.#randomBytes),
      }),
    );
  }

  #sendError(device: StoredDevice, requestId: string, code: string, message: string): void {
    this.#send(device, {
      type: "remote.error",
      id: randomPacketId(this.#randomBytes),
      sentAt: this.#now(),
      requestId,
      code,
      message,
    });
  }

  #onRelayClose(socket: WebSocket): void {
    if (socket !== this.#relay) return;
    this.#relay = undefined;
    for (const local of this.#localSockets.values()) local.close();
    this.#localSockets.clear();
    this.#options.onStatus?.("offline");
    if (!this.#desired) return;
    this.#reconnectTimer = setTimeout(() => {
      this.#reconnectTimer = undefined;
      this.#connect();
    }, RECONNECT_DELAY_MS);
  }

  #wasSeen(deviceId: string, packetId: string): boolean {
    return this.#seenPacketIds.get(deviceId)?.has(packetId) === true;
  }

  #rememberPacket(deviceId: string, packetId: string): void {
    const seen = this.#seenPacketIds.get(deviceId) ?? new Set<string>();
    seen.add(packetId);
    if (seen.size > 512) seen.delete(seen.values().next().value as string);
    this.#seenPacketIds.set(deviceId, seen);
  }

  #save(): Promise<void> {
    this.#saveQueue = this.#saveQueue.then(() => writeState(this.#options.statePath, this.#state));
    return this.#saveQueue;
  }
}

export function isAllowedRemoteRequest(method: "GET" | "POST", path: string): boolean {
  const pathname = new URL(path, "http://local").pathname;
  if (method === "GET") {
    return (
      pathname === "/api/v1/sessions" ||
      /^\/api\/v1\/sessions\/[^/]+\/(?:approvals|questions|transcript)$/.test(pathname)
    );
  }
  return /^\/api\/v1\/sessions\/[^/]+(?::abort|\/prompts(?::steer)?|\/approvals\/[^/]+|\/questions\/[^/]+(?::dismiss)?)$/.test(
    pathname,
  );
}

function isAllowedRemoteWsFrame(data: string): boolean {
  try {
    return remoteWsFrameSchema.safeParse(JSON.parse(data) as unknown).success;
  } catch {
    return false;
  }
}

async function loadState(statePath: string, randomBytes: RandomBytes): Promise<BridgeState> {
  try {
    const parsed = JSON.parse(await readFile(statePath, "utf8")) as BridgeState;
    if (
      parsed.version === 1 &&
      typeof parsed.runtimeId === "string" &&
      typeof parsed.publicKey === "string" &&
      typeof parsed.secretKey === "string" &&
      Array.isArray(parsed.devices)
    ) {
      return parsed;
    }
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code !== "ENOENT") throw error;
  }
  const identity = createRuntimeIdentity(randomBytes);
  const state: BridgeState = { version: 1, ...identity, devices: [] };
  await writeState(statePath, state);
  return state;
}

async function writeState(statePath: string, state: BridgeState): Promise<void> {
  await mkdir(dirname(statePath), { recursive: true, mode: 0o700 });
  const temporary = `${statePath}.tmp.${process.pid}`;
  await writeFile(temporary, JSON.stringify(state, null, 2), { mode: 0o600 });
  await chmod(temporary, 0o600);
  await rename(temporary, statePath);
}

function rawDataToString(data: RawData): string {
  if (Array.isArray(data)) return Buffer.concat(data).toString("utf8");
  if (data instanceof ArrayBuffer) return Buffer.from(new Uint8Array(data)).toString("utf8");
  return data.toString("utf8");
}
