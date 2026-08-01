import { DurableObject } from "cloudflare:workers";

import {
  relayFrameSchema,
  relayRegisterFrameSchema,
  type RelayDataFrame,
  type RelayRegisterFrame,
} from "@dimi-agent/remote";

const MAX_MESSAGE_SIZE = 2 << 20;
type Connection = RelayRegisterFrame & { readonly registered: boolean };

export class RelayRoom extends DurableObject {
  override fetch(request: Request): Response {
    const connection = parseConnection(request);
    if (request.headers.get("Upgrade") !== "websocket" || connection === undefined) {
      return new Response("Expected a valid WebSocket connection.", { status: 400 });
    }

    const pair = new WebSocketPair();
    const client = pair[0];
    const server = pair[1];
    this.ctx.acceptWebSocket(
      server,
      connection.role === "bridge"
        ? ["bridge"]
        : ["device", `device:${connection.deviceId}`],
    );
    server.serializeAttachment({ ...connection, registered: false } satisfies Connection);
    return new Response(null, { status: 101, webSocket: client });
  }

  override webSocketMessage(socket: WebSocket, message: string | ArrayBuffer): void {
    const connection = readConnection(socket);
    if (connection === undefined || typeof message !== "string") {
      socket.close(1003, "Invalid relay connection.");
      return;
    }
    if (message.length > MAX_MESSAGE_SIZE) {
      socket.close(1009, "Relay frame is too large.");
      return;
    }

    let value: unknown;
    try {
      value = JSON.parse(message);
    } catch {
      return;
    }
    const parsed = relayFrameSchema.safeParse(value);
    if (!parsed.success) return;
    const frame = parsed.data;

    if (!connection.registered) {
      if (frame.type !== "register" || !sameConnection(connection, frame)) {
        socket.close(1008, "Registration does not match the relay URL.");
        return;
      }
      const registered = { ...connection, registered: true } satisfies Connection;
      socket.serializeAttachment(registered);
      this.#register(socket, registered);
      return;
    }
    if (frame.type !== "data") return;
    this.#route(connection, frame);
  }

  override webSocketClose(socket: WebSocket): void {
    this.#disconnect(socket);
  }

  override webSocketError(socket: WebSocket): void {
    this.#disconnect(socket);
    socket.close(1011, "Relay connection failed.");
  }

  #register(socket: WebSocket, connection: Connection): void {
    if (connection.role === "bridge") {
      for (const previous of this.#registered("bridge")) {
        if (previous !== socket) previous.close(4001, "Replaced by a new bridge connection.");
      }
      for (const device of this.#registered("device")) {
        const deviceConnection = readConnection(device);
        if (deviceConnection?.role !== "device") continue;
        send(socket, { type: "presence", deviceId: deviceConnection.deviceId, connected: true });
        send(device, { type: "registered", role: "device", bridgeOnline: true });
      }
      send(socket, { type: "registered", role: "bridge", bridgeOnline: true });
      return;
    }

    for (const previous of this.#registered(`device:${connection.deviceId}`)) {
      if (previous !== socket) previous.close(4001, "Replaced by a new device connection.");
    }
    const bridge = this.#registered("bridge")[0];
    if (bridge !== undefined) {
      send(bridge, { type: "presence", deviceId: connection.deviceId, connected: true });
    }
    send(socket, { type: "registered", role: "device", bridgeOnline: bridge !== undefined });
  }

  #route(connection: Connection, frame: RelayDataFrame): void {
    if (connection.role === "device") {
      if (frame.deviceId !== connection.deviceId) return;
      const bridge = this.#registered("bridge")[0];
      if (bridge !== undefined) send(bridge, frame);
      return;
    }
    const device = this.#registered(`device:${frame.deviceId}`)[0];
    if (device !== undefined) send(device, frame);
  }

  #disconnect(socket: WebSocket): void {
    const connection = readConnection(socket);
    if (connection?.registered !== true) return;
    if (connection.role === "bridge") {
      if (this.#registered("bridge").some((candidate) => candidate !== socket)) return;
      for (const device of this.#registered("device")) {
        send(device, { type: "registered", role: "device", bridgeOnline: false });
      }
      return;
    }
    if (
      this.#registered(`device:${connection.deviceId}`).some((candidate) => candidate !== socket)
    ) {
      return;
    }
    const bridge = this.#registered("bridge")[0];
    if (bridge !== undefined) {
      send(bridge, { type: "presence", deviceId: connection.deviceId, connected: false });
    }
  }

  #registered(tag: string): WebSocket[] {
    return this.ctx
      .getWebSockets(tag)
      .filter(
        (socket) => socket.readyState === WebSocket.OPEN && readConnection(socket)?.registered,
      );
  }
}

function parseConnection(request: Request): RelayRegisterFrame | undefined {
  const url = new URL(request.url);
  const role = url.searchParams.get("role");
  const candidate = {
    type: "register",
    protocol: 1,
    role,
    runtimeId: url.searchParams.get("runtime"),
    ...(role === "device" ? { deviceId: url.searchParams.get("device") } : {}),
  };
  const parsed = relayRegisterFrameSchema.safeParse(candidate);
  return parsed.success ? parsed.data : undefined;
}

function readConnection(socket: WebSocket): Connection | undefined {
  const value: unknown = socket.deserializeAttachment();
  if (
    typeof value !== "object" ||
    value === null ||
    !("registered" in value) ||
    typeof value.registered !== "boolean"
  ) {
    return undefined;
  }
  const parsed = relayRegisterFrameSchema.safeParse(value);
  return parsed.success ? { ...parsed.data, registered: value.registered } : undefined;
}

function sameConnection(expected: Connection, actual: RelayRegisterFrame): boolean {
  return (
    expected.role === actual.role &&
    expected.runtimeId === actual.runtimeId &&
    (expected.role === "bridge" ||
      (actual.role === "device" && expected.deviceId === actual.deviceId))
  );
}

function send(socket: WebSocket, frame: object): void {
  if (socket.readyState === WebSocket.OPEN) socket.send(JSON.stringify(frame));
}

export default {
  fetch(request, env) {
    const url = new URL(request.url);
    if (url.pathname === "/health") return Response.json({ status: "ok" });
    const connection = parseConnection(request);
    if (connection === undefined) return new Response("Not found.", { status: 404 });
    return env.RELAY_ROOMS.getByName(connection.runtimeId).fetch(request);
  },
} satisfies ExportedHandler<Env>;
