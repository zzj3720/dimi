#!/usr/bin/env node
// Dimi Electron client — user-visible e2e suite (sequential runner).
// Drives the running app through CDP and asserts TUI-parity behavior from
// the user's perspective. Exits non-zero on the first failure.
//
// Usage: E2E_WS=<ws-url> node scripts/e2e.test.mjs

const wsUrl = process.env.E2E_WS;
if (!wsUrl) {
  console.error('usage: E2E_WS=<ws-url> node scripts/e2e.test.mjs');
  process.exit(1);
}

// ------------------------------------------------------------- CDP driver

class Cdp {
  constructor(url) {
    this.ws = new WebSocket(url);
    this.id = 0;
    this.pending = new Map();
  }
  async open() {
    this.ws.addEventListener('message', (evt) => {
      const msg = JSON.parse(evt.data.toString());
      if (msg.id && this.pending.has(msg.id)) {
        const { resolve, reject } = this.pending.get(msg.id);
        this.pending.delete(msg.id);
        msg.error ? reject(new Error(msg.error.message)) : resolve(msg.result);
      }
    });
    await new Promise((resolve, reject) => {
      this.ws.addEventListener('open', resolve, { once: true });
      this.ws.addEventListener('error', reject, { once: true });
    });
    await this.send('Runtime.enable');
  }
  send(method, params = {}) {
    return new Promise((resolve, reject) => {
      const msgId = ++this.id;
      const timer = setTimeout(() => {
        this.pending.delete(msgId);
        reject(new Error(`CDP ${method} timed out`));
      }, 15000);
      this.pending.set(msgId, {
        resolve: (v) => { clearTimeout(timer); resolve(v); },
        reject: (e) => { clearTimeout(timer); reject(e); },
      });
      this.ws.send(JSON.stringify({ id: msgId, method, params }));
    });
  }
  async eval(expression) {
    const res = await this.send('Runtime.evaluate', { expression, returnByValue: true, awaitPromise: true });
    if (res.exceptionDetails) throw new Error(res.exceptionDetails.text);
    return res.result?.value;
  }
  close() { try { this.ws.close(); } catch {} }
}

// ------------------------------------------------------------- helpers

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

let cdp;
let passed = 0;
let failed = 0;

async function test(name, fn) {
  try {
    await cleanup();
    await fn();
    passed += 1;
    console.log(`  ✅ ${name}`);
  } catch (e) {
    failed += 1;
    console.log(`  ❌ ${name}`);
    console.log(`     ${e.message}`);
  }
}

async function setInput(text) {
  return cdp.eval(`(async () => {
    const ta = document.querySelector('[data-testid="composer-input"]');
    if (!ta) throw new Error('composer input not found');
    ta.focus();
    ta.value = ${JSON.stringify(text)};
    ta.dispatchEvent(new Event('input', { bubbles: true }));
    await new Promise(r => setTimeout(r, 120));
    return true;
  })()`);
}

async function key(keyName, mods = {}) {
  return cdp.eval(`(async () => {
    const ta = document.querySelector('[data-testid="composer-input"]');
    if (!ta) throw new Error('composer input not found');
    ta.focus();
    const mod = ${JSON.stringify(mods)};
    ta.dispatchEvent(new KeyboardEvent('keydown', {
      key: ${JSON.stringify(keyName)},
      metaKey: !!mod.meta, ctrlKey: !!mod.ctrl, shiftKey: !!mod.shift, bubbles: true,
    }));
    await new Promise(r => setTimeout(r, 120));
    return true;
  })()`);
}

async function completionItems() {
  return cdp.eval(`[...document.querySelectorAll('[data-testid="completion-value"]')].map(e => e.textContent)`);
}

async function dialogItems() {
  return cdp.eval(`[...document.querySelectorAll('.dialog .list-item')].map(e => e.textContent)`);
}

async function fireSse(type, payload) {
  return cdp.eval(`(async () => {
    window.dispatchEvent(new CustomEvent('dimi:msg', { detail: { type: 'sse_event', evt: { payload: { type: ${JSON.stringify(type)}, ...${JSON.stringify(payload)} } } } }));
    await new Promise(r => setTimeout(r, 100));
    return true;
  })()`);
}

// The Vue renderer exposes `dispatch` on window as __dimiDispatch (api.ts):
// the pre-Vue renderer routed every message through the window 'dimi:msg'
// event (fireSse above), while the Vue renderer only consumes new_chat /
// suggestion_send there — so inject store messages through this hook.
async function injectSse(type, payload) {
  return cdp.eval(`(async () => {
    if (typeof window.__dimiDispatch !== 'function') throw new Error('__dimiDispatch not reachable');
    window.__dimiDispatch({ type: 'sse_event', evt: { payload: { type: ${JSON.stringify(type)}, ...${JSON.stringify(payload)} } } });
    await new Promise(r => setTimeout(r, 100));
    return true;
  })()`);
}

// Reset UI state between tests: close dialogs, clear draft, clear
// completion, dismiss any approval/question panel.
async function cleanup() {
  await cdp.eval(`(async () => {
    // Legacy pre-Vue bridge events — no-ops in the Vue renderer, harmless.
    window.dispatchEvent(new CustomEvent('dimi:msg', { detail: { type: 'approval_reject' } }));
    window.dispatchEvent(new CustomEvent('dimi:msg', { detail: { type: 'question_dismiss' } }));
    window.dispatchEvent(new CustomEvent('dimi:msg', { detail: { type: 'picker_close' } }));
    window.dispatchEvent(new CustomEvent('dimi:msg', { detail: { type: 'settings_close' } }));
    window.dispatchEvent(new CustomEvent('dimi:msg', { detail: { type: 'completion_close' } }));
    // Vue renderer: close layers through the store directly (approval_reject /
    // question_dismiss also fire the server-side reject/dismiss effects).
    if (typeof window.__dimiDispatch === 'function') {
      for (const t of ['approval_reject', 'question_dismiss', 'picker_close', 'settings_close', 'completion_close']) {
        window.__dimiDispatch({ type: t });
      }
    }
    const ta = document.querySelector('[data-testid="composer-input"]');
    if (ta) {
      ta.value = '';
      ta.dispatchEvent(new Event('input', { bubbles: true }));
    }
    await new Promise(r => setTimeout(r, 150));
    return true;
  })()`);
}

// ------------------------------------------------------------- run

async function main() {
  cdp = new Cdp(wsUrl);
  await cdp.open();
  console.log('Dimi client e2e suite\n');

  await test('boot: connected to server', async () => {
    const conn = await cdp.eval(`document.querySelector('#connection-badge')?.textContent`);
    if (conn !== 'connected') throw new Error(`expected connected, got ${conn}`);
  });

  await test('slash menu: /mo shows fuzzy command completion', async () => {
    await setInput('/mo');
    const items = await completionItems();
    if (!items.includes('/model')) throw new Error(`expected /model in ${JSON.stringify(items)}`);
    if (!items.includes('/secondary_model')) throw new Error(`expected /secondary_model in ${JSON.stringify(items)}`);
  });

  await test('slash menu: ↑↓ navigates, Enter accepts full command', async () => {
    await setInput('/mo');
    await key('ArrowDown');
    await key('Enter');
    const draft = await cdp.eval(`document.querySelector('#input').value`);
    if (draft !== '/secondary_model' && draft !== '/model') {
      throw new Error(`draft should be a full command, got ${draft}`);
    }
  });

  await test('slash menu: Tab accepts + reopens arg completion', async () => {
    await setInput('/per');
    await key('Tab');
    const draft = await cdp.eval(`document.querySelector('#input').value`);
    if (draft !== '/permission ') throw new Error(`expected '/permission ', got ${draft}`);
    const items = await completionItems();
    if (JSON.stringify(items) !== JSON.stringify(['manual', 'yolo', 'auto'])) {
      throw new Error(`expected arg completion, got ${JSON.stringify(items)}`);
    }
  });

  await test('slash menu: Esc closes the popup', async () => {
    await setInput('/mo');
    const open = await cdp.eval(`!document.querySelector('#completion').classList.contains('hidden')`);
    if (!open) throw new Error('popup should be open');
    await key('Escape');
    const closed = await cdp.eval(`document.querySelector('#completion').classList.contains('hidden')`);
    if (!closed) throw new Error('popup should be closed');
  });

  await test('@mention: lists local directory entries', async () => {
    await setInput('see @s');
    const items = await completionItems();
    if (items.length === 0) throw new Error(`expected directory entries, got ${JSON.stringify(items)}`);
  });

  await test('approval: SSE opens panel with TUI choices', async () => {
    await fireSse('event.approval.requested', {
      approval_id: 'e2e_approval', tool_name: 'Bash', action: 'run', tool_input_display: 'ls',
    });
    const items = await dialogItems();
    const expected = ['Approve once', 'Approve for this session', 'Reject', 'Reject with feedback…'];
    if (JSON.stringify(items) !== JSON.stringify(expected)) {
      throw new Error(`expected ${JSON.stringify(expected)}, got ${JSON.stringify(items)}`);
    }
    await fireSse('event.approval.resolved', { approval_id: 'e2e_approval' });
  });

  await test('approval: ↑↓ navigates, Esc rejects server-side', async () => {
    await fireSse('event.approval.requested', {
      approval_id: 'e2e_approval2', tool_name: 'Bash', action: 'run', tool_input_display: 'pwd',
    });
    await key('ArrowDown');
    const selected = await cdp.eval(`document.querySelector('.dialog .list-item.selected')?.textContent`);
    if (selected !== 'Approve for this session') throw new Error(`expected second choice, got ${selected}`);
    await key('Escape');
    const closed = await cdp.eval(`!document.querySelector('.dialog')`);
    if (!closed) throw new Error('approval should close after Esc');
  });

  await test('question: multi toggles with number keys', async () => {
    await fireSse('event.question.requested', {
      question_id: 'e2e_q', questions: [{
        id: 'q_0', question: 'Pick', multi_select: true,
        options: [{ id: 'a', label: 'Alpha' }, { id: 'b', label: 'Beta' }],
      }],
    });
    await key('1');
    await key('2');
    const items = await dialogItems();
    if (items[0] !== '✓ Alpha') throw new Error(`expected ✓ Alpha, got ${items[0]}`);
    if (items[1] !== '✓ Beta') throw new Error(`expected ✓ Beta, got ${items[1]}`);
    await key('Escape');
  });

  await test('question: single select advances to next tab', async () => {
    await fireSse('event.question.requested', {
      question_id: 'e2e_q2', questions: [
        { id: 'q_0', question: 'Q1', multi_select: false, options: [{ id: 'a', label: 'A' }, { id: 'b', label: 'B' }] },
        { id: 'q_1', question: 'Q2', multi_select: false, options: [{ id: 'c', label: 'C' }] },
      ],
    });
    await key('1');
    const title = await cdp.eval(`document.querySelector('.dialog .title')?.textContent`);
    if (title !== 'Q2') throw new Error(`expected Q2 after single select, got ${title}`);
    await key('Escape');
  });

  await test('session picker: opens and lists sessions', async () => {
    await cdp.eval(`document.querySelector('#btn-sessions').click()`);
    await sleep(700);
    const hasDialog = await cdp.eval(`!!document.querySelector('.dialog')`);
    if (!hasDialog) throw new Error('picker dialog should open');
    const count = await cdp.eval(`document.querySelectorAll('.dialog .list-item').length`);
    if (count === 0) throw new Error('expected sessions in picker');
    await key('Escape');
  });

  await test('session picker: Esc clears query then closes', async () => {
    await cdp.eval(`document.querySelector('#btn-sessions').click()`);
    await sleep(600);
    await cdp.eval(`(async () => {
      const s = document.querySelector('.search-input');
      s.focus(); s.value = 'rust'; s.dispatchEvent(new Event('input', { bubbles: true }));
      await new Promise(r => setTimeout(r, 80));
      return true;
    })()`);
    await key('Escape');
    const query = await cdp.eval(`document.querySelector('.search-input')?.value`);
    if (query !== '') throw new Error(`first Esc should clear query, got ${query}`);
    const stillOpen = await cdp.eval(`!!document.querySelector('.dialog')`);
    if (!stillOpen) throw new Error('dialog should stay open after first Esc');
    await key('Escape');
    const closed = await cdp.eval(`!document.querySelector('.dialog')`);
    if (!closed) throw new Error('second Esc should close');
  });

  await test('busy: steer button appears when busy', async () => {
    await fireSse('event.session.work_changed', { busy: true, main_turn_active: true });
    const visible = await cdp.eval(`!document.querySelector('#btn-steer').classList.contains('hidden')`);
    if (!visible) throw new Error('steer button should be visible when busy');
    await fireSse('event.session.work_changed', { busy: false, main_turn_active: false });
    const hidden = await cdp.eval(`document.querySelector('#btn-steer').classList.contains('hidden')`);
    if (!hidden) throw new Error('steer button should hide when idle');
  });

  await test('@mention: Enter accepts a single-@ value (no double @)', async () => {
    // Diff case: with draft '@src/ma' and item '@src/main.py', completion_accept
    // must produce '@src/main.py' — never '@@src/main.py' or '@src@src/…'.
    // Use '@./' + the first real cwd entry: the mention value is '@' + dir +
    // name, so the accepted draft must equal '@./' + the item label exactly.
    await setInput('@./');
    let label = null;
    for (let i = 0; i < 20; i++) {
      const items = await completionItems();
      if (items.length > 0) {
        label = items[0];
        break;
      }
      await sleep(100);
    }
    if (!label) throw new Error('mention popup did not open (empty cwd?)');
    await key('Enter');
    const draft = await cdp.eval(`document.querySelector('[data-testid="composer-input"]').value`);
    const expected = '@./' + label;
    if (draft !== expected) throw new Error(`expected draft '${expected}', got '${draft}'`);
    if (draft.startsWith('@@')) throw new Error(`double-@ regression: ${draft}`);
    const popupClosed = await cdp.eval(`!document.querySelector('[data-testid="completion"]')`);
    if (!popupClosed) throw new Error('completion popup should close after accept');
  });

  await test('approval: empty feedback on "Reject with feedback…" never POSTs', async () => {
    await injectSse('event.approval.requested', {
      approval_id: 'e2e_approval_empty', tool_name: 'Bash', action: 'run', tool_input_display: 'ls',
    });
    // Digit-4 shortcut selects "Reject with feedback…" and enters feedback mode.
    await key('4');
    const feedbackShown = await cdp.eval(`!!document.querySelector('input[placeholder="Feedback…"]')`);
    if (!feedbackShown) throw new Error('feedback input should be visible after selecting row 4');
    // Confirm while the feedback is still empty: the reducer + submitApproval
    // guards must keep the dialog open and surface 'enter feedback first'
    // instead of POSTing a feedback-less rejection (which would clear the
    // approval and close the panel).
    await key('Enter');
    const status = await cdp.eval(`document.querySelector('[data-testid="status-msg"]')?.textContent ?? ''`);
    if (status !== 'enter feedback first') throw new Error(`expected 'enter feedback first', got '${status}'`);
    const stillOpen = await cdp.eval(`!!document.querySelector('input[placeholder="Feedback…"]')`);
    if (!stillOpen) throw new Error('approval dialog should stay open when feedback is empty');
    // Clean up: resolve the approval so it doesn't leak into the next test.
    await injectSse('event.approval.resolved', { approval_id: 'e2e_approval_empty' });
  });

  console.log(`\n${passed} passed, ${failed} failed`);
  cdp.close();
  process.exit(failed === 0 ? 0 : 1);
}

main().catch((e) => {
  console.error('suite crashed:', e);
  cdp?.close();
  process.exit(1);
});
