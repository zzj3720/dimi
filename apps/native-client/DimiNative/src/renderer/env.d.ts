/// <reference types="vite/client" />

declare module '*.vue' {
  import type { DefineComponent } from 'vue';
  const component: DefineComponent<Record<string, unknown>, Record<string, unknown>, unknown>;
  export default component;
}

// Bridge exposed by the Electron main process (preload.mjs).
interface DimiBridge {
  request(opts: {
    method?: string;
    url: string;
    headers?: Record<string, string>;
    body?: unknown;
  }): Promise<{ status: number; ok: boolean; json: unknown; text: string }>;
  subscribeEvents(channel: string, url: string): void;
  stopEvents(channel: string): void;
  listFs(dir: string): Promise<{ ok: boolean; entries: unknown[]; error?: string }>;
  onEvent(channel: string, cb: (payload: unknown) => void): void;
  cwd?: string;
}

interface Window {
  dimi?: DimiBridge;
}
