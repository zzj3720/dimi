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

import { model, Msg, update, findSlashCommand, isBashDraft, APPROVAL_CHOICES } from './app.js';
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
      // Only actually abort when the reducer decided to cancel the stream
      // (first Ctrl+C with a non-empty draft just cleared the text).
      if (model.cancelStreamRequested) {
        model.cancelStreamRequested = false;
        doCancel();
      }
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
      // Double-Esc undo: when the reducer armed an undo, run it.
      if (model.undoRequested) {
        model.undoRequested = false;
        runUndo(1);
      }
      // Esc on an approval/question panel rejects/dismisses server-side.
      if (model.approvalRejectRequested) {
        model.approvalRejectRequested = false;
        rejectApproval();
      }
      if (model.questionDismissRequested) {
        model.questionDismissRequested = false;
        dismissQuestion();
      }
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

  // Question dialog: number keys select options, space toggles multi, Enter
  // confirms (TUI question-dialog.ts:161-194).
  if (model.currentQuestion) {
    if (/^[1-9]$/.test(evt.key)) {
      evt.preventDefault();
      const idx = Number(evt.key) - 1;
      const q = model.currentQuestion;
      if (idx < (q.options ?? []).length) {
        dispatch({ type: 'question_select', index: idx });
        if (q.kind !== 'multi' && q.kind !== 'multi_with_other') {
          dispatch(Msg.QuestionConfirm());
        }
      }
      return;
    }
    if (evt.key === ' ') {
      evt.preventDefault();
      dispatch({ type: 'question_toggle', index: model.questionSelectedIndex });
      return;
    }
  }

  // Ctrl / Cmd combos (platform shortcut).
  if (ctrlKey(evt)) {
    const k = evt.key.toLowerCase();
    if (k === 'c' || k === 'd') {
      // TUI layered Ctrl+C/Ctrl+D (editor-keyboard.ts:123-180): an open
      // approval/question panel consumes it first (reject/dismiss); then a
      // non-empty draft clears it (second press cancels); only an empty
      // draft + idle reaches the exit-confirm window.
      evt.preventDefault();
      if (model.currentApproval) {
        dispatch(Msg.ApprovalReject());
        return;
      }
      if (model.currentQuestion) {
        dispatch({ type: 'question_dismiss' });
        return;
      }
      dispatch(Msg.Cancel());
      return;
    }
    if (k === 's') { evt.preventDefault(); dispatch(Msg.Steer()); return; }
    if (k === 'o') { evt.preventDefault(); dispatch(Msg.ExpandToggle()); return; }
    if (k === 'g') { evt.preventDefault(); openExternalEditor(); return; }
    if (k === 't') { evt.preventDefault(); dispatch({ type: 'todo_toggle' }); return; }
    if (k === '-') { evt.preventDefault(); dispatch({ type: 'undo' }); return; }
    if (k === 'b') {
      // TUI onCtrlB: detach the current foreground task while streaming /
      // shell (idle + compacting fall through to nothing).
      if (model.busy && model.phase !== 'compacting') {
        evt.preventDefault();
        detachCurrentTask();
      }
      return;
    }
    return;
  }

  switch (evt.key) {
    case 'Enter':
      // Shift+Enter → newline; plain Enter → submit (TUI submit semantics).
      if (!evt.shiftKey) {
        evt.preventDefault();
        if (model.currentQuestion) {
          dispatch(Msg.QuestionConfirm());
        } else {
          dispatch(Msg.Submit());
        }
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

// 1s clock tick — drives the exit-confirm and double-Esc undo windows
// (TUI EXIT_CONFIRM_WINDOW_MS / DOUBLE_ESC_WINDOW_MS use real time).
setInterval(() => dispatch(Msg.Tick()), 1000);

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
    const p = evt?.payload ?? evt;
    const type = p?.type ?? evt?.type;
    // Keep the prompt queue in sync on turn/prompt lifecycle events
    // (TUI finalizeTurn drains one queued message per turn end).
    if (type === 'turn.ended' || type === 'prompt.completed' || type === 'prompt.steered') {
      refreshPromptQueue();
      // TUI drainOneQueuedMessage: after a turn ends, run the first queued
      // bash command (prompt messages drain via the server prompt queue).
      if (type === 'turn.ended' && !model.busy) {
        drainQueuedBash();
      }
    }
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
    // TUI (dimi-tui.ts:1007-1015): bash command while busy → enqueue with
    // mode='bash' (runs after the current task); idle → run immediately.
    if (model.busy) {
      model.queued.push({ text: draft, mode: 'bash' });
      model.statusMsg = `${model.queued.length} queued`;
    } else {
      runShellCommand(draft);
    }
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
    case 'compact':
      if (!model.currentSessionId) { model.statusMsg = 'select a session first'; render(); return; }
      api('POST', `/api/v1/sessions/${model.currentSessionId}:compact`, { instruction: resolved.args || undefined })
        .then(() => { model.statusMsg = 'compacting…'; render(); })
        .catch((e) => { model.statusMsg = `compact failed: ${e.message}`; render(); });
      break;
    case 'undo':
      runUndo(Number.parseInt(resolved.args, 10) || 1);
      break;
    case 'btw': {
      if (!model.currentSessionId) { model.statusMsg = 'select a session first'; render(); return; }
      model.btwOpen = true;
      model.btwBusy = true;
      model.btwPrompt = resolved.args ?? '';
      model.btwAnswer = '';
      model.statusMsg = 'starting btw…';
      render();
      api('POST', `/api/v1/sessions/${model.currentSessionId}:btw`, {})
        .then(async (data) => {
          const agentId = data?.data?.agent_id ?? 'main';
          model.btwAgentId = agentId;
          model.btwBusy = false;
          model.statusMsg = '';
          render();
          if (resolved.args && resolved.args.trim().length > 0) {
            await api('POST', `/api/v1/sessions/${model.currentSessionId}/prompts`, {
              content: [{ type: 'text', text: resolved.args }],
              agent_id: agentId,
            });
          }
        })
        .catch((e) => { model.statusMsg = `btw failed: ${e.message}`; render(); });
      break;
    }
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
  // TUI sendMessage routing (dimi-tui.ts:1369-1391), mapped onto the REST
  // prompts queue:
  //   - busy + steer mode → POST /prompts then immediately steer it into
  //     the running turn
  //   - busy + queue mode → POST /prompts (server queues it; runs when the
  //     current turn ends)
  //   - idle → POST /prompts (runs directly)
  try {
    const data = await api('POST', `/api/v1/sessions/${model.currentSessionId}/prompts`, {
      content: [{ type: 'text', text }],
    });
    const promptId = data?.data?.prompt_id ?? '';
    // TUI: isCompacting → enqueue, never steer (dimi-tui.ts:1370-1373).
    const steering = model.busy && model.busyInputMode === 'steer' && model.phase !== 'compacting' && promptId;
    if (steering) {
      await api('POST', `/api/v1/sessions/${model.currentSessionId}/prompts::steer`, {
        prompt_ids: [promptId],
      });
    }
    model.statusMsg = '';
    refreshPromptQueue();
    render();
  } catch (e) {
    model.statusMsg = `send failed: ${e.message}`;
    render();
  }
}

async function refreshPromptQueue() {
  if (!model.currentSessionId) return;
  try {
    const data = await api('GET', `/api/v1/sessions/${model.currentSessionId}/prompts`);
    const queued = data?.data?.queued ?? [];
    model.queued = queued.map((p) => ({ text: promptText(p), mode: 'prompt', promptId: p.prompt_id ?? '' }));
    render();
  } catch { /* non-fatal */ }
}

function promptText(p) {
  const parts = p?.content;
  if (typeof parts === 'string') return parts;
  if (Array.isArray(parts)) {
    return parts.map((x) => (x?.type === 'text' ? x.text : '')).filter(Boolean).join(' ');
  }
  return '';
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

// TUI drainOneQueuedMessage: after the current task ends, run the first
// queued bash command (dimi-tui.ts:1126-1137).
function drainQueuedBash() {
  const idx = model.queued.findIndex((q) => q.mode === 'bash');
  if (idx < 0) return;
  const item = model.queued[idx];
  model.queued.splice(idx, 1);
  runShellCommand(item.text);
  render();
}

function runUndo(count) {
  if (!model.currentSessionId) { model.statusMsg = 'select a session first'; render(); return; }
  api('POST', `/api/v1/sessions/${model.currentSessionId}:undo`, { count })
    .then((data) => {
      const msgs = data?.data?.messages?.items ?? [];
      model.statusMsg = `undone (${msgs.length} messages)`;
      // Reload the transcript to reflect the undo.
      connectSession(model.currentSessionId);
    })
    .catch((e) => { model.statusMsg = `undo failed: ${e.message}`; render(); });
}

function detachCurrentTask() {
  // TUI detachCurrentForegroundTask: cancel the current foreground action so
  // it keeps running in the background. On the REST surface the closest is
  // aborting the active prompt (which the engine then runs detached).
  if (!model.currentSessionId) return;
  api('POST', `/api/v1/sessions/${model.currentSessionId}:abort`, {})
    .then(() => { model.statusMsg = 'detached (running in background)'; render(); })
    .catch((e) => { model.statusMsg = `detach failed: ${e.message}`; render(); });
}

// ------------------------------------------------------------- steer/cancel

function doSteer() {
  if (model.phase !== 'streaming') return;
  // Ctrl+S: flush the queued prompts into the running turn
  // (TUI editor-keyboard.ts:249-314 — idle/shell/compacting are gated out).
  const ids = model.queued.map((q) => q.promptId).filter(Boolean);
  api('POST', `/api/v1/sessions/${model.currentSessionId}/prompts::steer`, { prompt_ids: ids })
    .then(() => {
      model.statusMsg = '';
      refreshPromptQueue();
      render();
    })
    .catch((e) => {
      model.statusMsg = `steer failed: ${e.message}`;
      render();
    });
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
  const idx = model.approvalSelectedIndex;
  let decision = 'approved';
  let scope;
  if (idx === 1) scope = 'session';
  else if (idx === 2) decision = 'rejected';
  else if (idx === 3) {
    // Reject with feedback
    api('POST', `/api/v1/sessions/${model.currentSessionId}/approvals/${a.id}`, {
      decision: 'rejected',
      feedback: model.approvalFeedbackText || undefined,
    })
      .then(() => { model.currentApproval = null; render(); })
      .catch((e) => { model.statusMsg = `approval failed: ${e.message}`; render(); });
    return;
  }
  api('POST', `/api/v1/sessions/${model.currentSessionId}/approvals/${a.id}`, {
    decision,
    scope,
    selected_label: APPROVAL_CHOICES[idx]?.label,
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

function dismissQuestion() {
  const q = model.currentQuestion;
  if (!q) return;
  // TUI: Esc → onAnswer({answers:[]}) which maps to the dismiss action.
  api('POST', `/api/v1/sessions/${model.currentSessionId}/questions/${q.id}:dismiss`, {})
    .catch(() => {})
    .finally(() => { model.currentQuestion = null; render(); });
}

function submitQuestion() {
  const q = model.currentQuestion;
  if (!q) return;
  const itemId = q.itemId || q.id;
  const selected = (q.options ?? []).filter((o) => o.selected).map((o) => o.id);
  const answers = {};
  if (q.kind === 'multi' && selected.length > 0) {
    answers[itemId] = { kind: 'multi', option_ids: selected };
  } else if (selected.length === 1) {
    answers[itemId] = { kind: 'single', option_id: selected[0] };
  } else if (q.questionOtherText && q.questionOtherText.trim().length > 0) {
    answers[itemId] = { kind: 'other', text: q.questionOtherText.trim() };
  } else {
    answers[itemId] = { kind: 'skipped' };
  }
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
