#!/usr/bin/env node
// CDP smoke check for the Dimi Electron client (uses Node's built-in WebSocket).
// Usage: node cdp-check.mjs <ws-url> [--eval "js"]
const wsUrl = process.argv[2];
const evalExpr = process.argv[3] === '--eval' ? process.argv[4] : null;
if (!wsUrl) { console.error('usage: node cdp-check.mjs <ws-url> [--eval "js"]'); process.exit(1); }

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
    if (msg.error) reject(new Error(msg.error.message));
    else resolve(msg.result);
  }
});

ws.addEventListener('open', async () => {
  try {
    await send('Runtime.enable');
    await new Promise((r) => setTimeout(r, 500));
    if (evalExpr) {
      const res = await send('Runtime.evaluate', { expression: evalExpr, returnByValue: true, awaitPromise: true });
      console.log(JSON.stringify(res.result?.value ?? res, null, 2));
    } else {
      const probe = await send('Runtime.evaluate', {
        expression: `JSON.stringify({
          connection: document.querySelector('#connection-badge')?.textContent,
          status: document.querySelector('#status-msg')?.textContent,
          transcriptEntries: document.querySelectorAll('#transcript .entry').length,
          inputValue: document.querySelector('#input')?.value,
          sessionsBtn: document.querySelector('#btn-sessions')?.textContent,
          hasDialog: !!document.querySelector('.dialog'),
        })`,
        returnByValue: true,
      });
      console.log(probe.result?.value);
    }
  } catch (e) {
    console.error('CDP error:', e.message);
  } finally {
    ws.close();
    process.exit(0);
  }
});

ws.addEventListener('error', (e) => { console.error('ws error:', e.message); process.exit(1); });
