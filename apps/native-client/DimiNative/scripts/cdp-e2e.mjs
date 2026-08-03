#!/usr/bin/env node
// End-to-end smoke: select session → verify message text → send a real
// message → wait for SSE → verify streaming assistant reply.
// Usage: node cdp-e2e.mjs <ws-url> [testMessage]
const wsUrl = process.argv[2];
const testMessage = process.argv[3] ?? 'hi, reply with just "pong" please';
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
    await new Promise((r) => setTimeout(r, 400));

    const evalJs = async (expr) => {
      const res = await send('Runtime.evaluate', { expression: expr, returnByValue: true, awaitPromise: true });
      return res.result?.value;
    };

    // 1. open picker, wait for sessions, select first
    const picker = await evalJs(`(async () => {
      document.querySelector('#btn-sessions').click();
      await new Promise(r=>setTimeout(r,800));
      const items = [...document.querySelectorAll('.dialog .list-item')];
      if (!items.length) return JSON.stringify({ok:false, step:'picker', reason:'no sessions'});
      items[0].dispatchEvent(new MouseEvent('mousedown', {bubbles:true}));
      await new Promise(r=>setTimeout(r,2000));
      const entries = [...document.querySelectorAll('#transcript .entry')];
      const firstBody = document.querySelector('#transcript .entry .body')?.textContent ?? '';
      return JSON.stringify({ok:true, dialogClosed: !document.querySelector('.dialog'), entryCount: entries.length, firstBodySample: firstBody.slice(0,60), bodyIsObject: firstBody === '[object Object]'});
    })()`);
    console.log('STEP1 picker+select:', picker);

    // 2. verify no [object Object]
    const probe = await evalJs(`JSON.stringify({conn: document.querySelector('#connection-badge')?.textContent, anyObject: [...document.querySelectorAll('#transcript .entry .body')].some(e => e.textContent === '[object Object]')})`);
    console.log('STEP2 content:', probe);

    // 3. send a real message
    const beforeCount = await evalJs(`document.querySelectorAll('#transcript .entry').length`);
    const sent = await evalJs(`(async () => {
      const ta = document.querySelector('#input');
      ta.focus();
      ta.value = ${JSON.stringify(testMessage)};
      ta.dispatchEvent(new Event('input', {bubbles:true}));
      await new Promise(r=>setTimeout(r,100));
      document.querySelector('#btn-send').click();
      await new Promise(r=>setTimeout(r,300));
      return JSON.stringify({draftCleared: ta.value === '', busy: !document.querySelector('#busy-badge').classList.contains('hidden')});
    })()`);
    console.log('STEP3 send:', sent, 'beforeCount=', beforeCount);

    // 4. wait for SSE streaming to render assistant text
    const result = await evalJs(`(async () => {
      const t0 = Date.now();
      let lastText = '';
      while (Date.now() - t0 < 30000) {
        const entries = [...document.querySelectorAll('#transcript .entry')];
        const last = entries[entries.length-1];
        if (last) {
          const body = last.querySelector('.body');
          if (body) {
            const txt = body.textContent;
            if (txt.length > 0 && txt !== lastText) lastText = txt;
          }
          if (entries.length > ${beforeCount} && lastText.length > 0) break;
        }
        await new Promise(r=>setTimeout(r,300));
      }
      const entries = [...document.querySelectorAll('#transcript .entry')];
      const lastBody = entries[entries.length-1]?.querySelector('.body')?.textContent ?? '';
      return JSON.stringify({
        elapsedMs: Date.now()-t0,
        totalEntries: entries.length,
        newEntries: entries.length - ${beforeCount},
        lastBodySample: lastBody.slice(0,120),
        busy: !document.querySelector('#busy-badge').classList.contains('hidden'),
        phase: document.querySelector('#phase-badge').textContent,
      });
    })()`);
    console.log('STEP4 stream:', result);
  } catch (e) {
    console.error('CDP error:', e.message);
  } finally {
    ws.close();
    process.exit(0);
  }
});

ws.addEventListener('error', (e) => { console.error('ws error:', e.message); process.exit(1); });
