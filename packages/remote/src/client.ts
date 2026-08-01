import {
  buildRelaySocketUrl,
  relayFrameSchema,
  type DeviceIdentity,
  type PairingDescriptor,
  type RemotePacket,
} from "./contract";
import { openPacket, randomPacketId, sealPacket, type RandomBytes } from "./crypto";

export type RemoteConnectionState = "disconnected" | "connecting" | "pairing" | "online";

export interface SocketMessage {
  readonly data: unknown;
}

export interface SocketLike {
  readonly readyState: number;
  onopen: (() => void) | null;
  onmessage: ((event: SocketMessage) => void) | null;
  onclose: (() => void) | null;
  onerror: (() => void) | null;
  send(data: string): void;
  close(): void;
}

export interface RemoteClientOptions {
  readonly pairing: PairingDescriptor;
  readonly identity: DeviceIdentity;
  readonly paired?: boolean;
  readonly randomBytes: RandomBytes;
  readonly createSocket: (url: string) => SocketLike;
  readonly now?: () => number;
  readonly onPaired?: () => void;
  readonly reconnectDelayMs?: (attempt: number) => number;
}

export interface RemoteHttpResponse<T = unknown> {
  readonly status: number;
  readonly body: T;
}

type StateListener = (state: RemoteConnectionState) => void;
type WsListener = (data: string) => void;
type PacketInput = RemotePacket extends infer Packet
  ? Packet extends RemotePacket
    ? Omit<Packet, "id" | "sentAt">
    : never
  : never;

export class RemoteClient {
  readonly #pairing: PairingDescriptor;
  readonly #identity: DeviceIdentity;
  readonly #randomBytes: RandomBytes;
  readonly #createSocket: (url: string) => SocketLike;
  readonly #now: () => number;
  readonly #onPaired?: () => void;
  readonly #reconnectDelayMs: (attempt: number) => number;
  readonly #stateListeners = new Set<StateListener>();
  readonly #wsListeners = new Set<WsListener>();
  readonly #pending = new Map<
    string,
    {
      resolve: (response: RemoteHttpResponse) => void;
      reject: (error: Error) => void;
      timer: ReturnType<typeof setTimeout>;
    }
  >();

  #socket?: SocketLike;
  #state: RemoteConnectionState = "disconnected";
  #paired: boolean;
  #desired = false;
  #reconnectAttempt = 0;
  #reconnectTimer?: ReturnType<typeof setTimeout>;

  constructor(options: RemoteClientOptions) {
    this.#pairing = options.pairing;
    this.#identity = options.identity;
    this.#paired = options.paired === true;
    this.#randomBytes = options.randomBytes;
    this.#createSocket = options.createSocket;
    this.#now = options.now ?? Date.now;
    this.#onPaired = options.onPaired;
    this.#reconnectDelayMs =
      options.reconnectDelayMs ?? ((attempt) => Math.min(1000 * 2 ** attempt, 15_000));
  }

  get state(): RemoteConnectionState {
    return this.#state;
  }

  start(): void {
    if (this.#desired) return;
    this.#desired = true;
    this.#connect();
  }

  close(): void {
    this.#desired = false;
    if (this.#reconnectTimer !== undefined) clearTimeout(this.#reconnectTimer);
    this.#reconnectTimer = undefined;
    this.#socket?.close();
    this.#socket = undefined;
    this.#rejectPending(new Error("Remote connection closed."));
    this.#setState("disconnected");
  }

  onState(listener: StateListener): () => void {
    this.#stateListeners.add(listener);
    listener(this.#state);
    return () => this.#stateListeners.delete(listener);
  }

  onWsMessage(listener: WsListener): () => void {
    this.#wsListeners.add(listener);
    return () => this.#wsListeners.delete(listener);
  }

  async request<T = unknown>(
    method: "GET" | "POST",
    path: string,
    body?: unknown,
  ): Promise<RemoteHttpResponse<T>> {
    if (this.#state !== "online") throw new Error("Remote runtime is offline.");
    const packet = this.#packet({ type: "http.request", method, path, body });
    const response = new Promise<RemoteHttpResponse>((resolve, reject) => {
      const timer = setTimeout(() => {
        this.#pending.delete(packet.id);
        reject(new Error(`Remote request timed out: ${method} ${path}`));
      }, 30_000);
      this.#pending.set(packet.id, { resolve, reject, timer });
    });
    this.#send(packet);
    return response as Promise<RemoteHttpResponse<T>>;
  }

  sendWs(data: string): void {
    if (this.#state !== "online") throw new Error("Remote runtime is offline.");
    this.#send(this.#packet({ type: "ws.send", data }));
  }

  #connect(): void {
    if (!this.#desired) return;
    this.#setState("connecting");
    const registration = {
      type: "register",
      protocol: 1,
      role: "device",
      runtimeId: this.#pairing.runtimeId,
      deviceId: this.#identity.deviceId,
    } as const;
    const socket = this.#createSocket(buildRelaySocketUrl(this.#pairing.relayUrl, registration));
    this.#socket = socket;
    socket.onopen = () => {
      socket.send(JSON.stringify(registration));
    };
    socket.onmessage = (event) => {
      this.#onMessage(event.data);
    };
    socket.onerror = () => {
      socket.close();
    };
    socket.onclose = () => {
      this.#onClose(socket);
    };
  }

  #onMessage(data: unknown): void {
    if (typeof data !== "string") return;
    let parsed: unknown;
    try {
      parsed = JSON.parse(data);
    } catch {
      return;
    }
    const result = relayFrameSchema.safeParse(parsed);
    if (!result.success) return;
    const frame = result.data;
    if (frame.type === "registered") {
      if (!frame.bridgeOnline) {
        this.#setState("disconnected");
        return;
      }
      if (!this.#paired) {
        if (this.#now() > this.#pairing.expiresAt) {
          this.#socket?.close();
          return;
        }
        this.#setState("pairing");
        this.#send(
          this.#packet({
            type: "pair.request",
            token: this.#pairing.token,
            deviceName: this.#identity.deviceName,
          }),
        );
      } else {
        this.#openLocalSocket();
      }
      return;
    }
    if (frame.type !== "data" || frame.deviceId !== this.#identity.deviceId) return;
    if (frame.publicKey !== this.#pairing.runtimePublicKey) return;
    try {
      this.#onPacket(
        openPacket(frame.payload, this.#pairing.runtimePublicKey, this.#identity.secretKey),
      );
    } catch {
      this.#socket?.close();
    }
  }

  #onPacket(packet: RemotePacket): void {
    if (packet.type === "pair.accepted") {
      this.#paired = true;
      this.#onPaired?.();
      this.#openLocalSocket();
      return;
    }
    if (packet.type === "ws.opened") {
      this.#reconnectAttempt = 0;
      this.#setState("online");
      return;
    }
    if (packet.type === "ws.message") {
      for (const listener of this.#wsListeners) listener(packet.data);
      return;
    }
    if (packet.type === "http.response") {
      const pending = this.#pending.get(packet.requestId);
      if (pending === undefined) return;
      clearTimeout(pending.timer);
      this.#pending.delete(packet.requestId);
      pending.resolve({ status: packet.status, body: packet.body });
      return;
    }
    if (packet.type === "remote.error") {
      const pending =
        packet.requestId === undefined ? undefined : this.#pending.get(packet.requestId);
      if (pending === undefined) return;
      clearTimeout(pending.timer);
      this.#pending.delete(packet.requestId as string);
      pending.reject(new Error(packet.message));
    }
  }

  #openLocalSocket(): void {
    const packet = this.#packet({ type: "ws.open" });
    this.#send(packet);
  }

  #send(packet: RemotePacket): void {
    const socket = this.#socket;
    if (socket === undefined || socket.readyState !== 1) {
      throw new Error("Relay connection is not open.");
    }
    socket.send(
      JSON.stringify({
        type: "data",
        deviceId: this.#identity.deviceId,
        publicKey: this.#identity.publicKey,
        payload: sealPacket(
          packet,
          this.#pairing.runtimePublicKey,
          this.#identity.secretKey,
          this.#randomBytes,
        ),
      }),
    );
  }

  #packet(packet: PacketInput): RemotePacket {
    return {
      ...packet,
      id: randomPacketId(this.#randomBytes),
      sentAt: this.#now(),
    } as RemotePacket;
  }

  #onClose(socket: SocketLike): void {
    if (socket !== this.#socket) return;
    this.#socket = undefined;
    this.#rejectPending(new Error("Remote runtime disconnected."));
    this.#setState("disconnected");
    if (!this.#desired) return;
    const delay = this.#reconnectDelayMs(this.#reconnectAttempt);
    this.#reconnectAttempt += 1;
    this.#reconnectTimer = setTimeout(() => {
      this.#reconnectTimer = undefined;
      this.#connect();
    }, delay);
  }

  #rejectPending(error: Error): void {
    for (const pending of this.#pending.values()) {
      clearTimeout(pending.timer);
      pending.reject(error);
    }
    this.#pending.clear();
  }

  #setState(state: RemoteConnectionState): void {
    if (state === this.#state) return;
    this.#state = state;
    for (const listener of this.#stateListeners) listener(state);
  }
}
