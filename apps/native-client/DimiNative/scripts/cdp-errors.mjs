#!/usr/bin/env node
// CDP console/exception capture. Usage: node cdp-errors.mjs <ws-url>
const wsUrl = process.argv[2];
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
  } else if (msg.method === 'Runtime.consoleAPICalled') {
    const args = msg.params.args.map((a) => a.value ?? a.description ?? '').join(' ');
    console.log(`[console.${msg.params.type}] ${args}`);
  } else if (msg.method === 'Runtime.exceptionThrown') {
    const d = msg.params.exceptionDetails;
    console.log('[EXCEPTION]', d.text, d.exception?.description ?? '');
  }
});

ws.addEventListener('open', async () => {
  try {
    await send('Runtime.enable');
    await new Promise((r) => setTimeout(r, 600));
    // Trigger a reload to capture boot errors.
    await send('Page.reload');
    await new Promise((r) => setTimeout(r, 2500));
  } catch (e) {
    console.error('CDP error:', e.message);
  } finally {
    ws.close();
    process.exit(0);
  }
});

ws.addEventListener('error', (e) => { console.error('ws error:', e.message); process.exit(1); });
