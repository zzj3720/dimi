import { z } from "zod";

export const REMOTE_PROTOCOL_VERSION = 1;
export const PAIRING_SCHEME = "k-3720:";

const idSchema = z.string().min(1).max(160);
const publicKeySchema = z.string().min(40).max(64);

export const relayRegisterFrameSchema = z.discriminatedUnion("role", [
  z.object({
    type: z.literal("register"),
    protocol: z.literal(REMOTE_PROTOCOL_VERSION),
    role: z.literal("bridge"),
    runtimeId: idSchema,
  }),
  z.object({
    type: z.literal("register"),
    protocol: z.literal(REMOTE_PROTOCOL_VERSION),
    role: z.literal("device"),
    runtimeId: idSchema,
    deviceId: idSchema,
  }),
]);

export const relayFrameSchema = z.discriminatedUnion("type", [
  relayRegisterFrameSchema,
  z.object({
    type: z.literal("registered"),
    role: z.enum(["bridge", "device"]),
    bridgeOnline: z.boolean(),
  }),
  z.object({
    type: z.literal("presence"),
    deviceId: idSchema,
    connected: z.boolean(),
  }),
  z.object({
    type: z.literal("data"),
    deviceId: idSchema,
    publicKey: publicKeySchema,
    payload: z.string().min(1),
  }),
  z.object({
    type: z.literal("error"),
    code: z.string().min(1),
    message: z.string().min(1),
  }),
]);

export type RelayFrame = z.infer<typeof relayFrameSchema>;
export type RelayRegisterFrame = z.infer<typeof relayRegisterFrameSchema>;
export type RelayDataFrame = Extract<RelayFrame, { type: "data" }>;

export function buildRelaySocketUrl(relayUrl: string, frame: RelayRegisterFrame): string {
  const url = new URL(relayUrl);
  url.searchParams.set("runtime", frame.runtimeId);
  url.searchParams.set("role", frame.role);
  if (frame.role === "device") url.searchParams.set("device", frame.deviceId);
  return url.toString();
}

const packetBase = {
  id: idSchema,
  sentAt: z.number().int().nonnegative(),
};

export const remotePacketSchema = z.discriminatedUnion("type", [
  z.object({
    ...packetBase,
    type: z.literal("pair.request"),
    token: z.string().min(20).max(160),
    deviceName: z.string().trim().min(1).max(80),
  }),
  z.object({
    ...packetBase,
    type: z.literal("pair.accepted"),
    requestId: idSchema,
    runtimeName: z.string().trim().min(1).max(80),
  }),
  z.object({
    ...packetBase,
    type: z.literal("http.request"),
    method: z.enum(["GET", "POST"]),
    path: z.string().startsWith("/api/v1/").max(2048),
    body: z.unknown().optional(),
  }),
  z.object({
    ...packetBase,
    type: z.literal("http.response"),
    requestId: idSchema,
    status: z.number().int().min(100).max(599),
    body: z.unknown(),
  }),
  z.object({
    ...packetBase,
    type: z.literal("ws.open"),
  }),
  z.object({
    ...packetBase,
    type: z.literal("ws.opened"),
    requestId: idSchema,
  }),
  z.object({
    ...packetBase,
    type: z.literal("ws.send"),
    data: z.string(),
  }),
  z.object({
    ...packetBase,
    type: z.literal("ws.message"),
    data: z.string(),
  }),
  z.object({
    ...packetBase,
    type: z.literal("ws.close"),
  }),
  z.object({
    ...packetBase,
    type: z.literal("remote.error"),
    requestId: idSchema.optional(),
    code: z.string().min(1),
    message: z.string().min(1),
  }),
]);

export type RemotePacket = z.infer<typeof remotePacketSchema>;
export type PairRequestPacket = Extract<RemotePacket, { type: "pair.request" }>;
export type HttpRequestPacket = Extract<RemotePacket, { type: "http.request" }>;

export const sealedPacketSchema = z.object({
  nonce: z.string().min(30).max(48),
  ciphertext: z.string().min(1),
});

export type SealedPacket = z.infer<typeof sealedPacketSchema>;

export interface PairingDescriptor {
  readonly relayUrl: string;
  readonly runtimeId: string;
  readonly runtimeName: string;
  readonly runtimePublicKey: string;
  readonly token: string;
  readonly expiresAt: number;
}

export interface DeviceIdentity {
  readonly deviceId: string;
  readonly deviceName: string;
  readonly publicKey: string;
  readonly secretKey: string;
}

export interface StoredRemote {
  readonly pairing: PairingDescriptor;
  readonly identity: DeviceIdentity;
  readonly paired: boolean;
}

export function buildPairingUri(descriptor: PairingDescriptor): string {
  const url = new URL(`${PAIRING_SCHEME}//pair`);
  url.searchParams.set("relay", descriptor.relayUrl);
  url.searchParams.set("runtime", descriptor.runtimeId);
  url.searchParams.set("name", descriptor.runtimeName);
  url.searchParams.set("key", descriptor.runtimePublicKey);
  url.searchParams.set("token", descriptor.token);
  url.searchParams.set("expires", String(descriptor.expiresAt));
  return url.toString();
}

export function parsePairingUri(value: string): PairingDescriptor {
  const url = new URL(value.trim());
  if (url.protocol !== PAIRING_SCHEME || url.hostname !== "pair" || url.pathname !== "") {
    throw new Error("Invalid pairing link.");
  }
  const relayUrl = requiredParam(url, "relay");
  const relayProtocol = new URL(relayUrl).protocol;
  if (relayProtocol !== "ws:" && relayProtocol !== "wss:") {
    throw new Error("Pairing relay must use ws or wss.");
  }
  const expiresAt = Number(requiredParam(url, "expires"));
  if (!Number.isSafeInteger(expiresAt)) throw new Error("Invalid pairing expiry.");
  return {
    relayUrl,
    runtimeId: requiredParam(url, "runtime"),
    runtimeName: requiredParam(url, "name"),
    runtimePublicKey: publicKeySchema.parse(requiredParam(url, "key")),
    token: requiredParam(url, "token"),
    expiresAt,
  };
}

function requiredParam(url: URL, name: string): string {
  const value = url.searchParams.get(name);
  if (value === null || value.length === 0) throw new Error(`Missing pairing field: ${name}.`);
  return value;
}
