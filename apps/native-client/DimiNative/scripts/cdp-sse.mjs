#!/usr/bin/env node
// Capture SSE events via the app's subscribeEvents bridge for N seconds.
// Usage: node cdp-sse.mjs <ws-url> <seconds>
const wsUrl = process.argv[2];
const seconds = Number(process.argv[3] ?? 8);
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
    await send('Runtime.enable');
    // Install a global SSE tap: intercept subscribeEvents calls.
    await send('Runtime.evaluate', {
      expression: `(() => {
        window.__sseEvents = [];
        const orig = window.dimi.subscribeEvents.bind(window.dimi);
        // We can't easily intercept the internal bridge, so instead hook the
        // dimi:msg dispatcher that receives SseEvent msgs.
        const origDispatch = window.dispatchEvent.bind(window);
        window.dispatchEvent = (evt) => {
          if (evt.detail && evt.detail.type === 'sse_event') {
            window.__sseEvents.push(evt.detail.evt);
          }
          return origDispatch(evt);
        };
        return true;
      })()`,
      returnByValue: true,
    });
    // Trigger a send.
    await send('Runtime.evaluate', {
      expression: `(async () => {
        const ta = document.querySelector('#input');
        ta.focus(); ta.value = 'capture test — reply with zzz';
        ta.dispatchEvent(new Event('input', { bubbles: true }));
        await new Promise(r => setTimeout(r, 120));
        document.querySelector('#btn-send').click();
        return true;
      })()`,
      awaitPromise: true,
      returnByValue: true,
    });
    await new Promise((r) => setTimeout(r, seconds * 1000));
    const res = await send('Runtime.evaluate', {
      expression: `JSON.stringify(window.__sseEvents.map(e => ({ type: e?.payload?.type ?? e?.type, agent: e?.payload?.agentId, prompt: e?.payload?.promptId, text: (e?.payload?.text ?? '').slice(0,40) })))`,
      returnByValue: true,
    });
    console.log(res.result?.value);
  } catch (e) {
    console.error('err', e.message);
  } finally {
    ws.close();
    process.exit(0);
  }
});
ws.addEventListener('error', (e) => { console.error('ws error', e.message); process.exit(1); });
