// Dimi native client — renderer entry. Binds DOM events → Msg, wires
// keyboard shortcuts (TUI §8), and drives the server transport.
//
// Keyboard semantics mirror the TUI (apps/dimi/src/tui/controllers/
// editor-keyboard.ts + custom-editor.ts):
//   Enter          submit (slash accept falls through to submit)
//   Tab            explicit completion trigger when menu closed
//   Esc            layered close (completion → picker → dialogs → cancel)
//   ↑/↓            completion / picker / approval / question / history
//   Ctrl+C         layered cancel → clear draft → double-press exit
//   Ctrl+D         same as Ctrl+C
//   Ctrl+S         steer (streaming only)
//   Ctrl+O         expand toggle
//   Ctrl+G         external editor
//   Ctrl+T         todo fold
//   Ctrl+-         undo
//   Shift+Tab      plan mode

import { model, Msg, update, findSlashCommand, isBashDraft } from './app.js';
import { render, els } from './view.js';

let serverUrl = 'http://127.0.0.1:58627';
if (typeof window.dimi !== 'undefined') {
  // The main process knows the real server URL; expose it for the cwd scope.
}
window.dimiCwd = '';

// ------------------------------------------------------------- dispatch

function dispatch(msg) {
  update(model, msg);
  render();
  afterDispatch(msg);
}

// Effects that must happen outside the reducer (network, focus).
function afterDispatch(msg) {
  switch (msg.type) {
    case 'session_selected':
      connectSession(msg.id);
      break;
    case 'picker_select': {
      // Handled in update; connect to the newly selected session.
      if (model.currentSessionId) connectSession(model.currentSessionId);
      break;
    }
    case 'submit':
      submitDraft();
      break;
    case 'steer':
      doSteer();
      break;
    case 'cancel':
      doCancel();
      break;
    case 'approval_confirm':
      submitApproval();
      break;
    case 'approval_reject':
      rejectApproval();
      break;
    case 'question_confirm':
      submitQuestion();
      break;
    case 'escape':
      // return focus to composer unless a dialog is open
      if (!model.pickerOpen && !model.currentApproval && !model.currentQuestion && !model.settingsDialogOpen) {
        els.input.focus();
      }
      break;
  }
}

// ------------------------------------------------------------- keyboard

const isMac = navigator.platform.startsWith('Mac');

function ctrlKey(evt) {
  return isMac ? evt.metaKey : evt.ctrlKey;
}

els.input.addEventListener('keydown', (evt) => {
  // Completion popup key handling first (it has priority while open).
  if (model.completionOpen) {
    if (evt.key === 'ArrowDown') { evt.preventDefault(); dispatch(Msg.CompletionMove(1)); return; }
    if (evt.key === 'ArrowUp') { evt.preventDefault(); dispatch(Msg.CompletionMove(-1)); return; }
    if (evt.key === 'Enter' || evt.key === 'Tab') {
      evt.preventDefault();
      dispatch(Msg.CompletionAccept());
      // TUI: accepting a slash command falls through to submit.
      const draft = model.draft;
      if (draft.startsWith('/') && findSlashCommand(draft.slice(1).split(/\s/)[0])) {
        dispatch(Msg.Submit());
      }
      return;
    }
    if (evt.key === 'Escape') { evt.preventDefault(); dispatch(Msg.CompletionClose()); return; }
  }

  // Ctrl / Cmd combos (platform shortcut).
  if (ctrlKey(evt)) {
    const k = evt.key.toLowerCase();
    if (k === 'c') { evt.preventDefault(); dispatch(Msg.Cancel()); return; }
    if (k === 'd') { evt.preventDefault(); dispatch(Msg.Cancel()); return; }
    if (k === 's') { evt.preventDefault(); dispatch(Msg.Steer()); return; }
    if (k === 'o') { evt.preventDefault(); dispatch(Msg.ExpandToggle()); return; }
    if (k === 'g') { evt.preventDefault(); openExternalEditor(); return; }
    if (k === 't') { evt.preventDefault(); dispatch({ type: 'todo_toggle' }); return; }
    if (k === '-') { evt.preventDefault(); dispatch({ type: 'undo' }); return; }
    return;
  }

  switch (evt.key) {
    case 'Enter':
      // Shift+Enter → newline; plain Enter → submit (TUI submit semantics).
      if (!evt.shiftKey) {
        evt.preventDefault();
        dispatch(Msg.Submit());
      }
      return;

    case 'Tab':
      // Explicit completion trigger when the menu is closed (TUI handleTabCompletion).
      evt.preventDefault();
      if (evt.shiftKey) {
        dispatch({ type: 'plan_mode_toggle' });
      } else {
        dispatch({ type: 'completion_tab' });
      }
      return;

    case 'Escape':
      evt.preventDefault();
      dispatch(Msg.Escape());
      return;

    case 'ArrowUp':
      evt.preventDefault();
      if (model.completionOpen) { dispatch(Msg.CompletionMove(-1)); return; }
      if (model.pickerOpen) { dispatch(Msg.PickerMove(-1)); return; }
      if (model.currentApproval) { dispatch(Msg.ApprovalMove(-1)); return; }
      if (model.currentQuestion) { dispatch(Msg.QuestionMove(-1)); return; }
      if (model.queued.length > 0 && model.draft.trim() === '') {
        recallLastQueued();
        return;
      }
      dispatch(Msg.HistoryPrev());
      return;

    case 'ArrowDown':
      evt.preventDefault();
      if (model.completionOpen) { dispatch(Msg.CompletionMove(1)); return; }
      if (model.pickerOpen) { dispatch(Msg.PickerMove(1)); return; }
      if (model.currentApproval) { dispatch(Msg.ApprovalMove(1)); return; }
      if (model.currentQuestion) { dispatch(Msg.QuestionMove(1)); return; }
      dispatch(Msg.HistoryNext());
      return;

    case 'Home':
    case 'End':
      // let the textarea handle caret movement natively
      return;

    default:
      return;
  }
});

els.input.addEventListener('input', () => {
  dispatch(Msg.DraftChange(els.input.value));
});

els.input.addEventListener('compositionend', () => {
  // After an IME commit the value may differ from the last input event;
  // resync so the model never drifts.
  dispatch(Msg.DraftChange(els.input.value));
});

// ------------------------------------------------------------- buttons

els.btnSend.addEventListener('click', () => dispatch(Msg.Submit()));
els.btnSessions.addEventListener('click', () => {
  dispatch(Msg.PickerOpen());
  loadSessions();
});
els.btnRefresh.addEventListener('click', () => loadSessions());
els.btnSteer.addEventListener('click', () => dispatch(Msg.SetBusyInputMode('steer')));
els.btnQueue.addEventListener('click', () => dispatch(Msg.SetBusyInputMode('queue')));
els.btnCancel.addEventListener('click', () => dispatch(Msg.Cancel()));

// Global message channel for view-generated events (dialog clicks etc.).
window.addEventListener('dimi:msg', (evt) => dispatch(evt.detail));

// ------------------------------------------------------------- server

export async function api(method, path, body) {
  const res = await window.dimi.request({ method, url: path, body });
  if (!res.ok) {
    throw new Error(`${method} ${path} -> ${res.status}: ${res.text?.slice(0, 200) ?? ''}`);
  }
  return res.json;
}

export async function loadSessions() {
  model.sessionsLoading = true;
  render();
  try {
    const data = await api('GET', '/api/v1/sessions?page_size=100');
    const items = data?.data?.items ?? [];
    dispatch(Msg.SessionsLoaded(items));
  } catch (e) {
    model.sessionsLoading = false;
    model.sessionsError = String(e);
    model.statusMsg = `failed to load sessions: ${e.message}`;
    render();
  }
}

export async function connectSession(sessionId) {
  // Load transcript baseline then subscribe to SSE.
  try {
    const data = await api('GET', `/api/v1/sessions/${sessionId}/messages?page_size=100`);
    const msgs = data?.data?.items ?? [];
    model.entries = msgsToEntries(msgs);
    model.entryCount = model.entries.length;
    render();
  } catch (e) {
    model.statusMsg = `failed to load messages: ${e.message}`;
    render();
  }
  subscribeSse(sessionId);
  fetchStatus(sessionId);
}

export async function fetchStatus(sessionId) {
  try {
    const data = await api('GET', `/api/v1/sessions/${sessionId}/status`);
    const st = data?.data;
    if (st) {
      model.busy = !!st.busy;
      model.phase = st.busy ? 'streaming' : 'idle';
      render();
    }
  } catch { /* non-fatal */ }
}

export function subscribeSse(sessionId) {
  if (model.sseUnsubscribe) {
    model.sseUnsubscribe();
    model.sseUnsubscribe = null;
  }
  model.sseUnsubscribe = window.dimi.subscribeEvents(`/api/v1/sessions/${sessionId}/events`, (evt) => {
    dispatch(Msg.SseEvent(evt));
  });
}

// ------------------------------------------------------------- submit

function submitDraft() {
  const draft = model.draft;
  if (draft.trim().length === 0) return;
  if (!model.currentSessionId) {
    model.statusMsg = 'select a session first';
    render();
    return;
  }

  // History push (TUI: every submitted non-empty draft joins history).
  if (model.inputHistory[model.inputHistory.length - 1] !== draft) {
    model.inputHistory.push(draft);
  }
  model.historyIndex = -1;

  if (isBashDraft(draft)) {
    runShellCommand(draft);
    model.draft = '';
    render();
    return;
  }

  if (draft.startsWith('/')) {
    const resolved = parseSlash(draft);
    if (resolved.known) {
      runSlashCommand(resolved);
      // TUI: accepted commands clear the composer even when busy-rejected
      // (idle_only rejection keeps it — see update on error).
      if (!model.lastCommandError) {
        model.draft = '';
        render();
      }
      return;
    }
    // Unknown /path → plain message (TUI dispatch fallback).
  }

  sendPrompt(draft);
  model.draft = '';
  render();
}

function parseSlash(text) {
  const body = text.slice(1);
  // Path form: contains '/' and no ':'.
  if (body.includes('/') && !body.includes(':')) {
    return { kind: 'path', name: text, known: false };
  }
  const m = text.match(/^\/(\S+)\s*([\s\S]*)$/);
  const name = m?.[1] ?? body;
  const args = m?.[2]?.trim() ?? '';
  const cmd = findSlashCommand(name);
  return { kind: 'command', name, args, command: cmd, known: cmd !== null };
}

function runSlashCommand(resolved) {
  model.lastCommandError = '';
  const cmd = resolved.command;
  if (cmd.idleOnly && model.busy) {
    model.lastCommandError = 'Cannot run this command while streaming — press Esc or Ctrl-C first.';
    render();
    return;
  }
  // Local commands the client can handle without the server.
  switch (cmd.name) {
    case 'help': model.statusMsg = 'help panel (coming)'; break;
    case 'exit': window.close(); break;
    case 'version': model.statusMsg = 'dimi client 0.1.0'; break;
    case 'sessions':
      dispatch(Msg.PickerOpen());
      loadSessions();
      break;
    case 'new': {
      // start a new session (server-side create)
      createSession().then((id) => {
        if (id) dispatch(Msg.SessionSelected(id));
      });
      break;
    }
    case 'theme': model.statusMsg = `theme ${resolved.args || 'auto'}`; break;
    case 'settings': dispatch(Msg.SettingsOpen()); break;
    case 'status': model.statusMsg = `status: session=${model.currentSessionId || '-'} busy=${model.busy} phase=${model.phase}`; break;
    case 'model': model.statusMsg = `model ${resolved.args || '(current)'} (coming)`; break;
    case 'permission': model.statusMsg = `permission ${resolved.args || '?'}`; break;
    case 'yolo': model.statusMsg = `yolo ${resolved.args || 'on'}`; break;
    case 'auto': model.statusMsg = `auto ${resolved.args || 'on'}`; break;
    case 'plan': model.statusMsg = `plan ${resolved.args || ''}`; break;
    case 'effort': model.statusMsg = `effort ${resolved.args || 'off'}`; break;
    case 'compact': model.statusMsg = 'compact (coming)'; break;
    case 'undo': model.statusMsg = 'undo (coming)'; break;
    case 'btw':
      model.btwOpen = true;
      model.btwDraft = resolved.args;
      model.btwPrompt = resolved.args;
      break;
    case 'usage': model.statusMsg = 'usage panel (coming)'; break;
    case 'tasks': dispatch(Msg.TasksOpen()); break;
    case 'copy': model.statusMsg = 'copy last assistant message'; break;
    default:
      model.statusMsg = `/${cmd.name} is not wired in this client yet.`;
  }
  render();
}

async function sendPrompt(text) {
  if (!model.currentSessionId) return;
  try {
    // Sending goes through the prompts route (REST): content is the parts
    // array with a single text part (mirror of the TUI's session.prompt).
    await api('POST', `/api/v1/sessions/${model.currentSessionId}/prompts`, {
      content: [{ type: 'text', text }],
    });
    model.statusMsg = '';
    render();
  } catch (e) {
    model.statusMsg = `send failed: ${e.message}`;
    render();
  }
}

function runShellCommand(text) {
  if (!model.currentSessionId) {
    model.statusMsg = 'select a session first';
    render();
    return;
  }
  api('POST', `/api/v1/sessions/${model.currentSessionId}/shell`, { command: text.slice(1).trim() })
    .then(() => { model.statusMsg = ''; render(); })
    .catch((e) => { model.statusMsg = `shell failed: ${e.message}`; render(); });
}

function recallLastQueued() {
  if (model.queued.length === 0) return;
  const last = model.queued.pop();
  model.draft = last.text;
  render();
  els.input.focus();
}

// ------------------------------------------------------------- steer/cancel

function doSteer() {
  if (model.phase !== 'streaming') return;
  model.statusMsg = 'steer (ctrl-s)';
  render();
}

function doCancel() {
  if (model.busy) {
    // Abort the active prompt: POST /sessions/{id}/prompts/{prompt_id}:abort
    // (the session carries current_prompt_id from the list/metadata).
    const pid = model.currentPromptId;
    if (!pid) {
      // Fall back to the session-level abort action when unknown.
      api('POST', `/api/v1/sessions/${model.currentSessionId}:abort`, {})
        .catch(() => {});
      return;
    }
    api('POST', `/api/v1/sessions/${model.currentSessionId}/prompts/${pid}:abort`, {})
      .catch(() => {});
  }
}

// ------------------------------------------------------------- approvals / questions

function submitApproval() {
  const a = model.currentApproval;
  if (!a) return;
  const opt = a.options?.[model.approvalSelectedIndex];
  api('POST', `/api/v1/sessions/${model.currentSessionId}/approvals/${a.id}`, {
    decision: 'approved',
    scope: 'session',
    selected_label: opt?.label,
  })
    .then(() => { model.currentApproval = null; render(); })
    .catch((e) => { model.statusMsg = `approval failed: ${e.message}`; render(); });
}

function rejectApproval() {
  const a = model.currentApproval;
  if (!a) return;
  api('POST', `/api/v1/sessions/${model.currentSessionId}/approvals/${a.id}`, {
    decision: 'rejected',
  })
    .then(() => { model.currentApproval = null; render(); })
    .catch(() => { model.currentApproval = null; render(); });
}

function submitQuestion() {
  const q = model.currentQuestion;
  if (!q) return;
  const answers = {};
  for (const o of q.options ?? []) {
    if (o.selected) {
      answers[q.id] = { kind: 'multi', option_ids: (q.options ?? []).filter((x) => x.selected).map((x) => x.id) };
      break;
    }
  }
  if (!answers[q.id]) answers[q.id] = { kind: 'skipped' };
  api('POST', `/api/v1/sessions/${model.currentSessionId}/questions/${q.id}`, { answers })
    .then(() => { model.currentQuestion = null; render(); })
    .catch((e) => { model.statusMsg = `question failed: ${e.message}`; render(); });
}

// ------------------------------------------------------------- helpers

function msgsToEntries(msgs) {
  return msgs.map((m) => ({
    kind: m.role === 'user' ? 'user' : m.role === 'assistant' ? 'assistant' : 'status',
    text: contentToText(m.content),
  }));
}

// MessageContent[] → plain text (text parts joined; tool parts flattened).
function contentToText(content) {
  if (typeof content === 'string') return content;
  if (!Array.isArray(content)) return '';
  return content
    .map((part) => {
      if (!part) return '';
      if (part.type === 'text') return part.text ?? '';
      if (part.type === 'thinking') return '';
      if (part.type === 'tool_use') return `[tool: ${part.tool_name}]`;
      if (part.type === 'tool_result') {
        return typeof part.output === 'string' ? part.output : JSON.stringify(part.output ?? '');
      }
      return '';
    })
    .filter((s) => s.length > 0)
    .join('\n');
}

async function createSession() {
  try {
    const data = await api('POST', '/api/v1/sessions', {});
    return data?.data?.id ?? data?.id ?? null;
  } catch (e) {
    model.statusMsg = `create failed: ${e.message}`;
    render();
    return null;
  }
}

function openExternalEditor() {
  model.statusMsg = 'external editor (coming)';
  render();
}

// ------------------------------------------------------------- boot

async function boot() {
  dispatch({ type: 'boot' });
  try {
    const meta = await api('GET', '/api/v1/meta');
    const serverVersion = meta?.data?.server_version ?? meta?.data?.version ?? meta?.serverVersion ?? '';
    const serverId = meta?.data?.server_id ?? meta?.data?.id ?? '';
    dispatch(Msg.SseConnected(serverVersion, serverId));
  } catch (e) {
    dispatch(Msg.SseError(String(e)));
  }
  loadSessions();
}

boot();
