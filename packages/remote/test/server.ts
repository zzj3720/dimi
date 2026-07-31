import { createServer, type Server } from "node:http";

import { relayFrameSchema, type RelayDataFrame } from "@k-3720/remote";
import { WebSocket, WebSocketServer, type RawData } from "ws";

interface Connection {
  role: "bridge" | "device";
  runtimeId: string;
  deviceId?: string;
}

export async function startRelay(): Promise<{
  readonly host: string;
  readonly port: number;
  close(): Promise<void>;
}> {
  const host = "127.0.0.1";
  const http = createServer();
  const sockets = new WebSocketServer({ server: http, maxPayload: 2 << 20 });
  const connections = new Map<WebSocket, Connection>();
  const bridges = new Map<string, WebSocket>();
  const devices = new Map<string, Map<string, WebSocket>>();

  sockets.on("connection", (socket) => {
    socket.on("message", (data) => {
      let value: unknown;
      try {
        value = JSON.parse(rawDataToString(data));
      } catch {
        return;
      }
      const parsed = relayFrameSchema.safeParse(value);
      if (!parsed.success) return;
      const frame = parsed.data;
      const connection = connections.get(socket);
      if (frame.type === "register") {
        if (connection !== undefined) return;
        connections.set(socket, {
          role: frame.role,
          runtimeId: frame.runtimeId,
          deviceId: frame.role === "device" ? frame.deviceId : undefined,
        });
        if (frame.role === "bridge") {
          bridges.get(frame.runtimeId)?.close();
          bridges.set(frame.runtimeId, socket);
          for (const [deviceId, device] of devices.get(frame.runtimeId) ?? []) {
            send(socket, { type: "presence", deviceId, connected: true });
            send(device, { type: "registered", role: "device", bridgeOnline: true });
          }
        } else {
          const runtimeDevices = devices.get(frame.runtimeId) ?? new Map<string, WebSocket>();
          runtimeDevices.get(frame.deviceId)?.close();
          runtimeDevices.set(frame.deviceId, socket);
          devices.set(frame.runtimeId, runtimeDevices);
          const bridge = bridges.get(frame.runtimeId);
          if (bridge !== undefined) {
            send(bridge, { type: "presence", deviceId: frame.deviceId, connected: true });
          }
        }
        send(socket, {
          type: "registered",
          role: frame.role,
          bridgeOnline: bridges.has(frame.runtimeId),
        });
        return;
      }
      if (connection === undefined || frame.type !== "data") return;
      routeData(connection, frame, bridges, devices);
    });

    socket.on("close", () => {
      const connection = connections.get(socket);
      connections.delete(socket);
      if (connection === undefined) return;
      if (connection.role === "bridge") {
        if (bridges.get(connection.runtimeId) === socket) bridges.delete(connection.runtimeId);
        for (const device of devices.get(connection.runtimeId)?.values() ?? []) {
          send(device, { type: "registered", role: "device", bridgeOnline: false });
        }
        return;
      }
      const runtimeDevices = devices.get(connection.runtimeId);
      const deviceId = connection.deviceId as string;
      if (runtimeDevices?.get(deviceId) === socket) runtimeDevices.delete(deviceId);
      const bridge = bridges.get(connection.runtimeId);
      if (bridge !== undefined) send(bridge, { type: "presence", deviceId, connected: false });
    });
  });

  await listen(http, host);
  const address = http.address();
  const port = typeof address === "object" && address !== null ? address.port : 0;
  return {
    host,
    port,
    async close() {
      for (const socket of sockets.clients) socket.terminate();
      await new Promise<void>((resolve) => {
        sockets.close(() => http.close(() => resolve()));
      });
    },
  };
}

function routeData(
  connection: Connection,
  frame: RelayDataFrame,
  bridges: ReadonlyMap<string, WebSocket>,
  devices: ReadonlyMap<string, ReadonlyMap<string, WebSocket>>,
): void {
  if (connection.role === "device") {
    if (frame.deviceId === connection.deviceId) bridges.get(connection.runtimeId)?.send(JSON.stringify(frame));
    return;
  }
  devices.get(connection.runtimeId)?.get(frame.deviceId)?.send(JSON.stringify(frame));
}

function send(socket: WebSocket, frame: object): void {
  if (socket.readyState === WebSocket.OPEN) socket.send(JSON.stringify(frame));
}

function listen(server: Server, host: string): Promise<void> {
  return new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, host, () => {
      server.off("error", reject);
      resolve();
    });
  });
}

function rawDataToString(data: RawData): string {
  if (Array.isArray(data)) return Buffer.concat(data).toString("utf8");
  if (data instanceof ArrayBuffer) return Buffer.from(new Uint8Array(data)).toString("utf8");
  return data.toString("utf8");
}
