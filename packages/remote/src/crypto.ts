import nacl from "tweetnacl";

import {
  remotePacketSchema,
  sealedPacketSchema,
  type DeviceIdentity,
  type RemotePacket,
  type SealedPacket,
} from "./contract";

const encoder = new TextEncoder();
const decoder = new TextDecoder();
const BASE64 = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

export type RandomBytes = (length: number) => Uint8Array;

export function createDeviceIdentity(deviceName: string, randomBytes: RandomBytes): DeviceIdentity {
  const keys = nacl.box.keyPair.fromSecretKey(randomBytes(nacl.box.secretKeyLength));
  const publicKey = bytesToBase64Url(keys.publicKey);
  return {
    deviceId: identityId("dev", keys.publicKey),
    deviceName,
    publicKey,
    secretKey: bytesToBase64Url(keys.secretKey),
  };
}

export function createRuntimeIdentity(randomBytes: RandomBytes): {
  runtimeId: string;
  publicKey: string;
  secretKey: string;
} {
  const keys = nacl.box.keyPair.fromSecretKey(randomBytes(nacl.box.secretKeyLength));
  return {
    runtimeId: identityId("rt", keys.publicKey),
    publicKey: bytesToBase64Url(keys.publicKey),
    secretKey: bytesToBase64Url(keys.secretKey),
  };
}

export function sealPacket(
  packet: RemotePacket,
  peerPublicKey: string,
  ownSecretKey: string,
  randomBytes: RandomBytes,
): string {
  const nonce = randomBytes(nacl.box.nonceLength);
  const ciphertext = nacl.box(
    encoder.encode(JSON.stringify(packet)),
    nonce,
    base64UrlToBytes(peerPublicKey),
    base64UrlToBytes(ownSecretKey),
  );
  return JSON.stringify({
    nonce: bytesToBase64Url(nonce),
    ciphertext: bytesToBase64Url(ciphertext),
  } satisfies SealedPacket);
}

export function openPacket(
  value: string,
  peerPublicKey: string,
  ownSecretKey: string,
): RemotePacket {
  const sealed = sealedPacketSchema.parse(JSON.parse(value));
  const plaintext = nacl.box.open(
    base64UrlToBytes(sealed.ciphertext),
    base64UrlToBytes(sealed.nonce),
    base64UrlToBytes(peerPublicKey),
    base64UrlToBytes(ownSecretKey),
  );
  if (plaintext === null) throw new Error("Remote packet authentication failed.");
  return remotePacketSchema.parse(JSON.parse(decoder.decode(plaintext)));
}

export function randomToken(randomBytes: RandomBytes, length = 24): string {
  return bytesToBase64Url(randomBytes(length));
}

export function randomPacketId(randomBytes: RandomBytes): string {
  return `msg_${bytesToBase64Url(randomBytes(16))}`;
}

export function bytesToBase64Url(bytes: Uint8Array): string {
  let result = "";
  for (let index = 0; index < bytes.length; index += 3) {
    const a = bytes[index] ?? 0;
    const b = bytes[index + 1] ?? 0;
    const c = bytes[index + 2] ?? 0;
    const remaining = bytes.length - index;
    result += BASE64[a >> 2];
    result += BASE64[((a & 3) << 4) | (b >> 4)];
    if (remaining > 1) result += BASE64[((b & 15) << 2) | (c >> 6)];
    if (remaining > 2) result += BASE64[c & 63];
  }
  return result;
}

export function base64UrlToBytes(value: string): Uint8Array {
  if (value.length % 4 === 1) throw new Error("Invalid base64url value.");
  const bytes: number[] = [];
  for (let index = 0; index < value.length; index += 4) {
    const a = decodeBase64(value[index]);
    const b = decodeBase64(value[index + 1]);
    const c = value[index + 2] === undefined ? 0 : decodeBase64(value[index + 2]);
    const d = value[index + 3] === undefined ? 0 : decodeBase64(value[index + 3]);
    bytes.push((a << 2) | (b >> 4));
    if (value[index + 2] !== undefined) bytes.push(((b & 15) << 4) | (c >> 2));
    if (value[index + 3] !== undefined) bytes.push(((c & 3) << 6) | d);
  }
  return Uint8Array.from(bytes);
}

function decodeBase64(value: string | undefined): number {
  if (value === undefined) throw new Error("Invalid base64url value.");
  const index = BASE64.indexOf(value);
  if (index < 0) throw new Error("Invalid base64url value.");
  return index;
}

function identityId(prefix: "dev" | "rt", publicKey: Uint8Array): string {
  return `${prefix}_${bytesToBase64Url(nacl.hash(publicKey).slice(0, 12))}`;
}
