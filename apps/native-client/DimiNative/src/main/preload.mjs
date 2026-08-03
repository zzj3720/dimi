// Dimi native client — preload. Exposes a minimal, explicit bridge to the
// renderer. The renderer holds all UI + logic; main is transport only.
import { contextBridge, ipcRenderer } from 'electron';

const api = {
  // HTTP(S) request via the main process (no CORS, token injected).
  request: (options) => ipcRenderer.invoke('http-request', options),

  // Subscribe to server-sent events. Returns an unsubscribe function.
  subscribeEvents: (url, onEvent) => {
    const channel = `sse:${Math.random().toString(36).slice(2)}`;
    const listener = (_evt, payload) => onEvent(payload);
    ipcRenderer.on(channel, listener);
    ipcRenderer.invoke('sse-start', { channel, url }).catch((e) => onEvent({ type: 'sse-error', message: String(e) }));
    return () => {
      ipcRenderer.removeListener(channel, listener);
      ipcRenderer.invoke('sse-stop', { channel }).catch(() => {});
    };
  },
};

contextBridge.exposeInMainWorld('dimi', api);
