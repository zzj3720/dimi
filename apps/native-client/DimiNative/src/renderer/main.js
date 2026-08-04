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
      if (model.approvalRejectRequested) {
        model.approvalRejectRequested = false;
        rejectApproval();
      }
      break;

    case 'question_dismiss':
      if (model.questionDismissRequested) {
        model.questionDismissRequested = false;
        dismissQuestion();
      }
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
      // Esc on a busy stream cancels it (TUI editor-keyboard.ts:195-204).
      if (model.cancelStreamRequested) {
        model.cancelStreamRequested = false;
        doCancel();
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
      const acceptViaTab = evt.key === 'Tab';
      dispatch(Msg.CompletionAccept());
      const draft = model.draft;
      if (draft.startsWith('/') && findSlashCommand(draft.slice(1).split(/\s/)[0])) {
        if (acceptViaTab) {
          // TUI: Tab-accept inserts a trailing space and reopens argument
          // completion (custom-editor.ts:580-592) so subcommands show.
          dispatch(Msg.DraftChange(draft + ' '));
        } else {
          // TUI: Enter-accept falls through to submit.
          dispatch(Msg.Submit());
        }
      }
      return;
    }
    if (evt.key === 'Escape') { evt.preventDefault(); dispatch(Msg.CompletionClose()); return; }
  }

  // Question dialog: number keys select options, space toggles multi, Enter
  // confirms, ←/→/Tab switch tabs (TUI question-dialog.ts:161-194).
  if (model.currentQuestion) {
    if (/^[1-9]$/.test(evt.key)) {
      evt.preventDefault();
      const idx = Number(evt.key) - 1;
      const q = model.currentQuestion;
      if (idx < (q.options ?? []).length) {
        if (q.kind === 'multi' || q.kind === 'multi_with_other') {
          // TUI: number key on multi toggles the option (question-dialog.ts:184-194).
          dispatch({ type: 'question_toggle', index: idx });
        } else {
          // TUI: single selection advances to the next unanswered question
          // or the Submit tab (question-dialog.ts:358-363), not submit-all.
          dispatch({ type: 'question_select', index: idx });
          dispatch(Msg.QuestionTab(1));
        }
      }
      return;
    }
    if (evt.key === ' ') {
      evt.preventDefault();
      dispatch({ type: 'question_toggle', index: model.questionSelectedIndex });
      return;
    }
    if (evt.key === 'ArrowLeft') { evt.preventDefault(); dispatch(Msg.QuestionTab(-1)); return; }
    if (evt.key === 'ArrowRight' || evt.key === 'Tab') { evt.preventDefault(); dispatch(Msg.QuestionTab(1)); return; }
  }

  // Approval dialog: number keys select+confirm (TUI approval-panel.ts:313-317);
  // index 3 (Reject with feedback) enters feedback mode instead of submitting.
  if (model.currentApproval) {
    if (/^[1-9]$/.test(evt.key)) {
      evt.preventDefault();
      const idx = Number(evt.key) - 1;
      if (idx < APPROVAL_CHOICES.length) {
        dispatch(Msg.ApprovalSelect(idx));
        if (idx === 3) {
          model.approvalFeedbackMode = true;
          render();
        } else {
          dispatch(Msg.ApprovalConfirm());
        }
      }
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
    if (k === 'e') {
      // TUI: Ctrl+E on the approval panel toggles diff/file preview
      // (approval-panel.ts:271-277).
      if (model.currentApproval) {
        evt.preventDefault();
        model.approvalPreview = !model.approvalPreview;
        render();
      }
      return;
    }
    if (k === 'a') {
      // TUI: Ctrl+A in the session picker toggles cwd/all scope
      // (session-picker.ts:172-174).
      if (model.pickerOpen) {
        evt.preventDefault();
        dispatch(Msg.PickerScope(model.pickerScope === 'cwd' ? 'all' : 'cwd'));
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
        if (model.currentApproval) {
          dispatch(Msg.ApprovalConfirm());
        } else if (model.currentQuestion) {
          // TUI: Enter activates the cursor option; single advances to the
          // next tab, multi toggles.
          const q = model.currentQuestion;
          const idx = model.questionSelectedIndex;
          if (q.kind === 'multi' || q.kind === 'multi_with_other') {
            dispatch({ type: 'question_toggle', index: idx });
          } else {
            if ((q.options ?? []).length > 0) {
              dispatch({ type: 'question_select', index: idx });
              dispatch(Msg.QuestionTab(1));
            }
          }
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
  // @mention completion is async (server fs browse); trigger it here.
  if (els.input.value.includes('@')) {
    maybeUpdateAtMention(els.input.value);
  } else {
    model.atMentionOpen = false;
    render();
  }
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
window.addEventListener('dimi:msg', (evt) => {
  const msg = evt.detail;
  if (msg.type === 'picker_load_more') {
    loadMoreSessions();
    return;
  }
  if (msg.type === 'btw_send') {
    sendBtw(msg.text);
    return;
  }
  if (msg.type === 'settings_set_model') {
    api('POST', `/api/v1/models/${encodeURIComponent(msg.ref)}:set_default`, {})
      .then((data) => { model.statusMsg = `default model → ${data?.data?.default_model ?? msg.ref}`; render(); })
      .catch((e) => { model.statusMsg = `model set failed: ${e.message}`; render(); });
    return;
  }
  if (msg.type === 'settings_set_permission') {
    api('POST', `/api/v1/config`, { default_permission_mode: msg.mode })
      .then(() => { model.statusMsg = `permission mode → ${msg.mode}`; render(); })
      .catch((e) => { model.statusMsg = `permission failed: ${e.message}`; render(); });
    return;
  }
  if (msg.type === 'settings_set_effort') {
    api('POST', `/api/v1/config`, { thinking: { effort: msg.effort } })
      .then(() => { model.statusMsg = `thinking effort → ${msg.effort}`; render(); })
      .catch((e) => { model.statusMsg = `effort failed: ${e.message}`; render(); });
    return;
  }
  if (msg.type === 'settings_set_theme') {
    applyTheme(msg.theme);
    return;
  }
  dispatch(msg);
});

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
    // TUI session-picker: pageSize=50, pages load on scroll (page 1 here).
    const data = await api('GET', '/api/v1/sessions?page_size=50');
    const items = data?.data?.items ?? [];
    model.sessions = items;
    model.sessionsHasMore = !!data?.data?.has_more;
    dispatch(Msg.SessionsLoaded(items));
  } catch (e) {
    model.sessionsLoading = false;
    model.sessionsError = String(e);
    model.statusMsg = `failed to load sessions: ${e.message}`;
    render();
  }
}

export async function loadMoreSessions() {
  if (model.sessionsLoading || !model.sessionsHasMore) return;
  model.sessionsLoading = true;
  render();
  const last = model.sessions[model.sessions.length - 1];
  try {
    const data = await api('GET', `/api/v1/sessions?page_size=50&before_id=${encodeURIComponent(last?.id ?? '')}`);
    const items = data?.data?.items ?? [];
    model.sessions = model.sessions.concat(items);
    model.sessionsHasMore = !!data?.data?.has_more;
    dispatch(Msg.SessionsLoaded(model.sessions));
  } catch (e) {
    model.sessionsLoading = false;
    model.statusMsg = `load more failed: ${e.message}`;
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
    }
    dispatch(Msg.SseEvent(evt));
    // TUI drainOneQueuedMessage: after a turn ends, run the first queued
    // bash command. Check AFTER the reducer processed turn.ended (which
    // clears busy) — checking before would see busy still true.
    if (type === 'turn.ended' && !model.busy) {
      setTimeout(() => {
        if (!model.busy) drainQueuedBash();
      }, 0);
    }
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
    case 'help':
      model.statusMsg = '';
      model.helpDialogOpen = true;
      render();
      break;
    case 'exit': window.close(); break;
    case 'version': model.statusMsg = 'Dimi Client 0.1.0'; break;
    case 'sessions':
      dispatch(Msg.PickerOpen());
      loadSessions();
      break;
    case 'new': {
      createSession().then((id) => {
        if (id) dispatch(Msg.SessionSelected(id));
      });
      break;
    }
    case 'theme':
      applyTheme(resolved.args || 'auto');
      break;
    case 'settings': dispatch(Msg.SettingsOpen()); break;
    case 'status': {
      const s = model.sessions.find((x) => x.id === model.currentSessionId);
      model.statusMsg = `session=${model.currentSessionId || '-'} · busy=${model.busy} · phase=${model.phase} · title=${s?.title ?? ''}`;
      break;
    }
    case 'copy': {
      // Copy the last assistant message to the clipboard (TUI dispatch).
      for (let i = model.entries.length - 1; i >= 0; i--) {
        if (model.entries[i].kind === 'assistant') {
          navigator.clipboard.writeText(model.entries[i].text).then(() => {
            model.statusMsg = 'copied last assistant message';
            render();
          });
          break;
        }
      }
      break;
    }
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
    case 'model': {
      // /model [name] — with an arg, set the default model
      // (POST /models/{provider/model}:set_default); without, show current.
      if (resolved.args) {
        const ref = resolved.args;
        api('POST', `/api/v1/models/${encodeURIComponent(ref)}:set_default`, {})
          .then((data) => {
            model.statusMsg = `default model → ${data?.data?.default_model ?? ref}`;
            render();
          })
          .catch((e) => { model.statusMsg = `model set failed: ${e.message}`; render(); });
      } else {
        api('GET', `/api/v1/config`)
          .then((data) => {
            const c = data?.data ?? {};
            model.statusMsg = `default model: ${c.default_model ?? '(unset)'}`;
            render();
          })
          .catch((e) => { model.statusMsg = `config failed: ${e.message}`; render(); });
      }
      break;
    }
    case 'permission': {
      const mode = resolved.args || 'manual';
      if (!['manual', 'auto', 'yolo'].includes(mode)) {
        model.statusMsg = `permission: manual|auto|yolo (got ${mode})`;
        render();
        return;
      }
      api('POST', `/api/v1/config`, { default_permission_mode: mode })
        .then(() => { model.statusMsg = `permission mode → ${mode}`; render(); })
        .catch((e) => { model.statusMsg = `permission failed: ${e.message}`; render(); });
      break;
    }
    case 'yolo': {
      const on = resolved.args !== 'off';
      api('POST', `/api/v1/config`, { yolo: on })
        .then(() => { model.statusMsg = `yolo ${on ? 'on' : 'off'}`; render(); })
        .catch((e) => { model.statusMsg = `yolo failed: ${e.message}`; render(); });
      break;
    }
    case 'auto': {
      const on = resolved.args !== 'off';
      api('POST', `/api/v1/config`, { default_permission_mode: on ? 'auto' : 'manual' })
        .then(() => { model.statusMsg = `auto ${on ? 'on' : 'off'}`; render(); })
        .catch((e) => { model.statusMsg = `auto failed: ${e.message}`; render(); });
      break;
    }
    case 'plan': {
      const on = resolved.args !== 'off' && resolved.args !== 'clear';
      api('POST', `/api/v1/config`, { default_plan_mode: on })
        .then(() => { model.statusMsg = `plan mode ${on ? 'on' : 'off'}`; render(); })
        .catch((e) => { model.statusMsg = `plan failed: ${e.message}`; render(); });
      break;
    }
    case 'effort': {
      const effort = resolved.args || 'off';
      api('POST', `/api/v1/config`, { thinking: { effort } })
        .then(() => { model.statusMsg = `thinking effort → ${effort}`; render(); })
        .catch((e) => { model.statusMsg = `effort failed: ${e.message}`; render(); });
      break;
    }
    case 'usage': {
      if (!model.currentSessionId) { model.statusMsg = 'select a session first'; render(); return; }
      api('GET', `/api/v1/sessions/${model.currentSessionId}/status`)
        .then((data) => {
          const st = data?.data ?? {};
          model.statusMsg = `context ${st.context_tokens ?? '?'}/${st.max_context_tokens ?? '?'} (${st.context_usage ?? '?'}%) · thinking ${st.thinking_level ?? '?'} · plan ${st.plan_mode ?? '?'}`;
          render();
        })
        .catch((e) => { model.statusMsg = `usage failed: ${e.message}`; render(); });
      break;
    }
    case 'tasks': dispatch(Msg.TasksOpen()); break;
    case 'fork': {
      if (!model.currentSessionId) { model.statusMsg = 'select a session first'; render(); return; }
      api('POST', `/api/v1/sessions/${model.currentSessionId}:fork`, {})
        .then((data) => {
          const id = data?.data?.id ?? '';
          model.statusMsg = id ? `forked ${id}` : 'forked';
          render();
        })
        .catch((e) => { model.statusMsg = `fork failed: ${e.message}`; render(); });
      break;
    }
    case 'title': {
      if (!model.currentSessionId) { model.statusMsg = 'select a session first'; render(); return; }
      if (!resolved.args) {
        const s = model.sessions.find((x) => x.id === model.currentSessionId);
        model.statusMsg = `title: ${s?.title ?? '(untitled)'}`;
        render();
        return;
      }
      api('POST', `/api/v1/sessions/${model.currentSessionId}/profile`, { title: resolved.args })
        .then(() => { model.statusMsg = `renamed to ${resolved.args}`; render(); })
        .catch((e) => { model.statusMsg = `rename failed: ${e.message}`; render(); });
      break;
    }
    case 'export-md': {
      if (!model.currentSessionId) { model.statusMsg = 'select a session first'; render(); return; }
      api('POST', `/api/v1/sessions/${model.currentSessionId}/export`, {})
        .then((data) => {
          const text = typeof data?.data === 'string' ? data.data : JSON.stringify(data?.data ?? {});
          navigator.clipboard.writeText(text).then(() => {
            model.statusMsg = 'export copied to clipboard';
            render();
          });
        })
        .catch((e) => { model.statusMsg = `export failed: ${e.message}`; render(); });
      break;
    }
    case 'reload': {
      // Reload the session: rebuild transcript baseline + reconnect SSE.
      if (model.currentSessionId) {
        connectSession(model.currentSessionId);
        model.statusMsg = 'reloaded';
      } else {
        model.statusMsg = 'no session to reload';
      }
      render();
      break;
    }
    default:
      model.statusMsg = `/${cmd.name} is not wired in this client yet.`;
  }
  render();
}

function applyTheme(theme) {
  model.theme = theme;
  const root = document.documentElement;
  if (theme === 'dark') {
    root.style.setProperty('--bg', '#1e1e1e');
    root.style.setProperty('--surface', '#252526');
    root.style.setProperty('--text', '#d4d4d4');
  } else if (theme === 'light') {
    root.style.setProperty('--bg', '#ffffff');
    root.style.setProperty('--surface', '#f3f3f3');
    root.style.setProperty('--text', '#1e1e1e');
  }
  // 'auto' follows the OS scheme (default dark for the POC).
  model.statusMsg = `theme ${theme}`;
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

// BTW side-chat: send a message to the btw agent and surface its reply.
function sendBtw(text) {
  if (!model.currentSessionId) return;
  const agentId = model.btwAgentId || 'main';
  model.btwPrompt = text;
  model.btwAnswer = '';
  model.btwBusy = true;
  render();
  api('POST', `/api/v1/sessions/${model.currentSessionId}/prompts`, {
    content: [{ type: 'text', text }],
    agent_id: agentId,
  })
    .then(() => { render(); })
    .catch((e) => {
      model.btwBusy = false;
      model.btwAnswer = `error: ${e.message}`;
      render();
    });
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
  // TUI onCtrlS (editor-keyboard.ts:249-314): flush queued prompts AND the
  // current draft (non-bash) into the running turn, then clear the editor.
  const draft = model.draft.trim();
  const draftIsBash = isBashDraft(draft);
  const submitDraftThenSteer = async () => {
    const ids = [];
    if (!draftIsBash && draft.length > 0) {
      try {
        const data = await api('POST', `/api/v1/sessions/${model.currentSessionId}/prompts`, {
          content: [{ type: 'text', text: draft }],
        });
        const pid = data?.data?.prompt_id ?? '';
        if (pid) ids.push(pid);
      } catch (e) {
        model.statusMsg = `steer failed: ${e.message}`;
        render();
        return;
      }
    }
    // Queued prompt ids (bash entries stay queued — TUI keeps them).
    for (const q of model.queued) {
      if (q.mode !== 'bash' && q.promptId) ids.push(q.promptId);
    }
    if (ids.length === 0) return;
    await api('POST', `/api/v1/sessions/${model.currentSessionId}/prompts::steer`, { prompt_ids: ids });
    if (!draftIsBash && draft.length > 0) model.draft = '';
    model.statusMsg = '';
    refreshPromptQueue();
    render();
  };
  submitDraftThenSteer().catch((e) => {
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
  // Collect answers for every question (TUI review page submits all tabs).
  const answers = {};
  const all = q.allQuestions ?? [q];
  for (const qq of all) {
    const itemId = qq.itemId || qq.id;
    const selected = (qq.options ?? []).filter((o) => o.selected).map((o) => o.id);
    const otherText = (qq.otherText ?? '').trim();
    if ((qq.kind === 'multi' || qq.kind === 'multi_with_other') && selected.length > 0 && otherText.length > 0) {
      answers[itemId] = { kind: 'multi_with_other', option_ids: selected, other_text: otherText };
    } else if ((qq.kind === 'multi' || qq.kind === 'multi_with_other') && selected.length > 0) {
      answers[itemId] = { kind: 'multi', option_ids: selected };
    } else if (selected.length === 1) {
      answers[itemId] = { kind: 'single', option_id: selected[0] };
    } else if (qq.allowOther && otherText.length > 0) {
      answers[itemId] = { kind: 'other', text: otherText };
    } else {
      answers[itemId] = { kind: 'skipped' };
    }
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

// @mention completion (TUI file-mention-provider): extract the `@<prefix>`
// after whitespace, list the local directory via the main process, filter
// and show in the same completion popup.
async function maybeUpdateAtMention(text) {
  const m = text.match(/(?:^|\s)@([^\s]*)$/);
  if (!m) { model.atMentionOpen = false; render(); return; }
  const prefix = m[1];
  try {
    // Directory part = everything before the last '/', or the cwd.
    const slashIdx = prefix.lastIndexOf('/');
    const dir = slashIdx >= 0 ? prefix.slice(0, slashIdx + 1) : (window.dimiCwd || '.');
    const namePrefix = slashIdx >= 0 ? prefix.slice(slashIdx + 1) : prefix;
    const res = await window.dimi.fsList(dir || '.');
    if (!res?.ok) { model.atMentionOpen = false; render(); return; }
    const entries = res.entries ?? [];
    const filtered = entries
      .filter((e) => e.name.toLowerCase().startsWith(namePrefix.toLowerCase()))
      .map((e) => ({
        value: `@${dir}${e.name}${e.isDirectory ? '/' : ''}`,
        label: `${e.name}${e.isDirectory ? '/' : ''}`,
        description: e.isDirectory ? undefined : e.path,
      }));
    if (filtered.length === 0) { model.atMentionOpen = false; render(); return; }
    model.atMentionOpen = true;
    // Position of '@' in the draft: the prefix starts after it.
    model.atMentionPrefix = text.length - prefix.length - 1;
    model.completionItems = filtered;
    model.completionSelected = 0;
    render();
  } catch {
    model.atMentionOpen = false;
    render();
  }
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
