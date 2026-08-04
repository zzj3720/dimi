// Dimi native client — Electron main process.
// Owns the window, the server connection (REST + SSE), and bridges
// everything to the renderer via contextBridge. All business logic lives
// in the renderer; the main process is only a transport + window shell.

import { app, BrowserWindow, ipcMain, shell } from 'electron';
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
    backgroundColor: '#141414', // matches the renderer app background (was #1e1e1e → gray flash on resize)
    // Codex-style hidden title bar: the macOS traffic lights float over the
    // custom 46px header, which owns -webkit-app-region:drag + an 88px safe
    // left inset (HeaderBar.styles.ts) exactly like the codex shell.
    titleBarStyle: 'hiddenInset',
    trafficLightPosition: { x: 14, y: 17 }, // vertically centered in the 46px header
    webPreferences: {
      preload: path.join(__dirname, 'preload.mjs'),
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: false,
    },
  });

  mainWindow.loadFile(path.join(__dirname, '../../dist/renderer/index.html'));

  // Open external http(s) links in the system browser instead of navigating
  // the app window away or spawning a child window.
  mainWindow.webContents.setWindowOpenHandler(({ url }) => {
    if (/^https?:/i.test(url)) shell.openExternal(url);
    return { action: 'deny' };
  });
  mainWindow.webContents.on('will-navigate', (e, url) => {
    const current = mainWindow.webContents.getURL();
    if (url !== current) {
      e.preventDefault();
      if (/^https?:/i.test(url)) shell.openExternal(url);
    }
  });

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

// --- IPC: list directory entries for @mention completion (local fs).
// The renderer is sandboxed (contextIsolation) so the main process reads
// the current working directory. Semantic close to the TUI's fd file
// search for @ mentions.

import { readdirSync, statSync } from 'node:fs';

ipcMain.handle('fs-list', (_evt, { dir = process.cwd() } = {}) => {
  try {
    const entries = readdirSync(dir, { withFileTypes: true })
      .map((d) => ({
        name: d.name,
        isDirectory: d.isDirectory(),
        path: `${dir}/${d.name}`,
      }))
      .sort((a, b) => (a.isDirectory === b.isDirectory ? a.name.localeCompare(b.name) : a.isDirectory ? -1 : 1));
    return { ok: true, entries };
  } catch (e) {
    return { ok: false, error: String(e) };
  }
});

// --- IPC: native file picker + multipart upload for the attachment button.
// dialog is main-process-only; upload uses FormData so the JSON http-request
// bridge cannot carry it.

import { dialog } from 'electron';
import { readFileSync } from 'node:fs';
import { basename } from 'node:path';

ipcMain.handle('pick-files', async () => {
  const result = await dialog.showOpenDialog({ properties: ['openFile', 'multiSelections'] });
  if (result.canceled) return { ok: true, paths: [] };
  return { ok: true, paths: result.filePaths };
});

ipcMain.handle('upload-file', async (_evt, { path: filePath } = {}) => {
  try {
    const buf = readFileSync(filePath);
    const form = new FormData();
    form.append('file', new Blob([buf]), basename(filePath));
    const fullUrl = `${serverUrl}/api/v1/files`;
    const resp = await fetch(fullUrl, {
      method: 'POST',
      headers: serverToken ? { Authorization: `Bearer ${serverToken}` } : {},
      body: form,
    });
    const text = await resp.text();
    let json = null;
    try { json = text ? JSON.parse(text) : null; } catch { /* keep null */ }
    return { status: resp.status, ok: resp.ok, json, text };
  } catch (e) {
    return { status: 0, ok: false, json: null, text: String(e) };
  }
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
          try {
            wc.send(channel, JSON.parse(payload));
          } catch { wc.send(channel, { type: 'raw', data: payload }); }
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
