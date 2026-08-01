import { SELF } from "cloudflare:test";
import {
  buildRelaySocketUrl,
  relayFrameSchema,
  type RelayFrame,
  type RelayRegisterFrame,
} from "@k-3720/remote";
import { describe, expect, it } from "vitest";

describe("Cloudflare relay", () => {
  it("reports health", async () => {
    const response = await SELF.fetch("https://relay.test/health");
    expect(response.status).toBe(200);
    await expect(response.json()).resolves.toEqual({ status: "ok" });
  });

  it("isolates runtimes and forwards opaque frames in both directions", async () => {
    const runtimeA = "runtime-a";
    const runtimeB = "runtime-b";
    const deviceId = "device-1";
    const bridgeA = await connect(registerBridge(runtimeA));
    const bridgeB = await connect(registerBridge(runtimeB));
    const deviceA = await connect(registerDevice(runtimeA, deviceId));
    const deviceB = await connect(registerDevice(runtimeB, deviceId));

    const toBridgeA = dataFrame(deviceId, "device-a-to-bridge-a");
    const toBridgeB = dataFrame(deviceId, "device-b-to-bridge-b");
    const receivedByA = waitForFrame(bridgeA, (frame) => frame.type === "data");
    const receivedByB = waitForFrame(bridgeB, (frame) => frame.type === "data");
    deviceA.send(JSON.stringify(toBridgeA));
    deviceB.send(JSON.stringify(toBridgeB));
    await expect(receivedByA).resolves.toEqual(toBridgeA);
    await expect(receivedByB).resolves.toEqual(toBridgeB);

    const toDeviceA = dataFrame(deviceId, "bridge-a-to-device-a");
    const toDeviceB = dataFrame(deviceId, "bridge-b-to-device-b");
    const receivedByDeviceA = waitForFrame(deviceA, (frame) => frame.type === "data");
    const receivedByDeviceB = waitForFrame(deviceB, (frame) => frame.type === "data");
    bridgeA.send(JSON.stringify(toDeviceA));
    bridgeB.send(JSON.stringify(toDeviceB));
    await expect(receivedByDeviceA).resolves.toEqual(toDeviceA);
    await expect(receivedByDeviceB).resolves.toEqual(toDeviceB);

    bridgeA.close();
    bridgeB.close();
    deviceA.close();
    deviceB.close();
  });

  it("replaces connections and reports presence without a false offline transition", async () => {
    const runtimeId = "runtime-reconnect";
    const deviceId = "device-reconnect";
    const firstBridge = await connect(registerBridge(runtimeId));
    const device = await connect(registerDevice(runtimeId, deviceId));
    const oldBridgeClosed = waitForClose(firstBridge);
    const deviceStillOnline = waitForFrame(
      device,
      (frame) => frame.type === "registered" && frame.bridgeOnline,
    );
    const secondBridge = await connect(registerBridge(runtimeId));

    await expect(oldBridgeClosed).resolves.toMatchObject({ code: 4001 });
    await expect(deviceStillOnline).resolves.toMatchObject({
      type: "registered",
      bridgeOnline: true,
    });

    const disconnected = waitForFrame(
      secondBridge,
      (frame) =>
        frame.type === "presence" && frame.deviceId === deviceId && !frame.connected,
    );
    device.close();
    await expect(disconnected).resolves.toMatchObject({ connected: false });
    secondBridge.close();
  });
});

async function connect(registration: RelayRegisterFrame): Promise<WebSocket> {
  const response = await SELF.fetch(buildRelaySocketUrl("https://relay.test", registration), {
    headers: { Upgrade: "websocket" },
  });
  expect(response.status).toBe(101);
  const socket = response.webSocket;
  if (socket === null) throw new Error("Relay did not return a WebSocket.");
  socket.accept();
  const registered = waitForFrame(socket, (frame) => frame.type === "registered");
  socket.send(JSON.stringify(registration));
  await registered;
  return socket;
}

function waitForFrame(
  socket: WebSocket,
  predicate: (frame: RelayFrame) => boolean,
): Promise<RelayFrame> {
  return new Promise((resolve, reject) => {
    const timeout = setTimeout(() => {
      reject(new Error("Timed out waiting for relay frame."));
    }, 5_000);
    const listener = (event: MessageEvent): void => {
      const parsed = relayFrameSchema.safeParse(JSON.parse(String(event.data)));
      if (!parsed.success) return;
      const frame = parsed.data;
      if (!predicate(frame)) return;
      clearTimeout(timeout);
      socket.removeEventListener("message", listener);
      resolve(frame);
    };
    socket.addEventListener("message", listener);
  });
}

function waitForClose(socket: WebSocket): Promise<CloseEvent> {
  return new Promise((resolve, reject) => {
    const timeout = setTimeout(() => {
      reject(new Error("Timed out waiting for close."));
    }, 5_000);
    socket.addEventListener("close", (event) => {
      clearTimeout(timeout);
      resolve(event);
    });
  });
}

function registerBridge(runtimeId: string): RelayRegisterFrame {
  return { type: "register", protocol: 1, role: "bridge", runtimeId };
}

function registerDevice(runtimeId: string, deviceId: string): RelayRegisterFrame {
  return { type: "register", protocol: 1, role: "device", runtimeId, deviceId };
}

function dataFrame(deviceId: string, payload: string): RelayFrame {
  return { type: "data", deviceId, publicKey: "p".repeat(40), payload };
}
