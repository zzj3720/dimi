// Dimi native client — Electron main process.
// Owns the window, the server connection (REST + SSE), and bridges
// everything to the renderer via contextBridge. All business logic lives
// in the renderer; the main process is only a transport + window shell.

import { app, BrowserWindow, ipcMain } from 'electron';
import { fileURLToPath } from 'node:url';
import path from 'node:path';

const __dirname = path.dirname(fileURLToPath(import.meta.url));

const DEFAULT_SERVER = 'http://127.0.0.1:58627';
const serverUrl = process.env.DIMI_SERVER_URL ?? process.env.DIMI_POC_SERVER_URL ?? DEFAULT_SERVER;
const serverToken = process.env.DIMI_TOKEN ?? process.env.DIMI_POC_TOKEN ?? '';

let mainWindow = null;

function createWindow() {
  mainWindow = new BrowserWindow({
    width: 1024,
    height: 720,
    minWidth: 640,
    minHeight: 400,
    title: 'Dimi Client',
    backgroundColor: '#1e1e1e',
    webPreferences: {
      preload: path.join(__dirname, 'preload.mjs'),
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: false,
    },
  });

  mainWindow.loadFile(path.join(__dirname, '../renderer/index.html'));
  mainWindow.on('closed', () => { mainWindow = null; });
}

// --- IPC: renderer asks the main process to perform HTTP(S) requests.
// Main-process net avoids CORS entirely and keeps the token out of the
// renderer's document origin.

ipcMain.handle('http-request', async (_evt, { method = 'GET', url, headers = {}, body } = {}) => {
  const fullUrl = url.startsWith('http') ? url : `${serverUrl}${url}`;
  const h = { ...headers };
  if (serverToken) h.Authorization = `Bearer ${serverToken}`;
  const resp = await fetch(fullUrl, {
    method,
    headers: { 'Content-Type': 'application/json', ...h },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const text = await resp.text();
  let json = null;
  try { json = text ? JSON.parse(text) : null; } catch { /* keep null */ }
  return { status: resp.status, ok: resp.ok, json, text };
});

// --- IPC: SSE stream. The main process owns the fetch stream and forwards
// parsed event lines to the renderer on the requested channel. This keeps a
// single live stream per channel and survives renderer reloads poorly (the
// renderer resubscribes on load, which is fine for the POC).

const sseStreams = new Map(); // channel -> { controller, webContents }

ipcMain.handle('sse-start', async (evt, { channel, url }) => {
  // Stop any existing stream on this channel first (idempotent resubscribe).
  stopSse(channel);
  const fullUrl = url.startsWith('http') ? url : `${serverUrl}${url}`;
  const controller = new AbortController();
  const wc = evt.sender;
  sseStreams.set(channel, { controller, webContents: wc });

  (async () => {
    try {
      const resp = await fetch(fullUrl, {
        headers: { Authorization: serverToken ? `Bearer ${serverToken}` : undefined },
        signal: controller.signal,
      });
      if (!resp.ok || !resp.body) {
        wc.send(channel, { type: 'sse-error', status: resp.status });
        return;
      }
      const reader = resp.body.getReader();
      const decoder = new TextDecoder();
      let buf = '';
      while (true) {
        const { done, value } = await reader.read();
        if (done) break;
        buf += decoder.decode(value, { stream: true });
        let idx;
        while ((idx = buf.indexOf('\n')) >= 0) {
          const line = buf.slice(0, idx).trim();
          buf = buf.slice(idx + 1);
          if (!line.startsWith('data:')) continue;
          const payload = line.slice(5).trim();
          if (!payload) continue;
          try { wc.send(channel, JSON.parse(payload)); } catch { wc.send(channel, { type: 'raw', data: payload }); }
        }
      }
      wc.send(channel, { type: 'sse-ended', outcome: 'ok' });
    } catch (e) {
      if (!controller.signal.aborted) {
        wc.send(channel, { type: 'sse-error', message: String(e) });
      }
    } finally {
      if (sseStreams.get(channel)?.controller === controller) sseStreams.delete(channel);
    }
  })();

  return true;
});

ipcMain.handle('sse-stop', (_evt, { channel }) => {
  stopSse(channel);
  return true;
});

function stopSse(channel) {
  const entry = sseStreams.get(channel);
  if (entry) {
    entry.controller.abort();
    sseStreams.delete(channel);
  }
}

app.whenReady().then(() => {
  createWindow();
  app.on('activate', () => {
    if (BrowserWindow.getAllWindows().length === 0) createWindow();
  });
});

app.on('window-all-closed', () => {
  if (process.platform !== 'darwin') app.quit();
});
