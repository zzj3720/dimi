#!/usr/bin/env node
// Capture a screenshot of the running app via CDP.
// Usage: node cdp-shot.mjs <ws-url> <outfile>
const wsUrl = process.argv[2];
const outfile = process.argv[3] ?? '/tmp/dimi-shot.png';
const ws = new WebSocket(wsUrl);
let id = 0;
const pending = new Map();

function send(method, params = {}) {
  return new Promise((resolve, reject) => {
    const msgId = ++id;
    pending.set(msgId, { resolve, reject });
    ws.send(JSON.stringify({ id: msgId, method, params }));
  });
}

ws.addEventListener('message', (evt) => {
  const msg = JSON.parse(evt.data.toString());
  if (msg.id && pending.has(msg.id)) {
    const { resolve, reject } = pending.get(msg.id);
    pending.delete(msg.id);
    msg.error ? reject(new Error(msg.error.message)) : resolve(msg.result);
  }
});

ws.addEventListener('open', async () => {
  try {
    await send('Page.enable');
    const res = await send('Page.captureScreenshot', { format: 'png' });
    const { writeFileSync } = await import('node:fs');
    writeFileSync(outfile, Buffer.from(res.data, 'base64'));
    console.log('saved', outfile);
  } catch (e) {
    console.error('err', e.message);
  } finally {
    ws.close();
    process.exit(0);
  }
});
ws.addEventListener('error', (e) => { console.error('ws error', e.message); process.exit(1); });
