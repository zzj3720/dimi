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
  subscribeEvents(url: string, onEvent: (payload: unknown) => void): () => void;
  fsList(dir: string): Promise<{ ok: boolean; entries?: { name: string; isDirectory: boolean; path: string }[]; error?: string }>;
}

interface Window {
  dimi?: DimiBridge;
}
