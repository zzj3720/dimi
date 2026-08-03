// Dimi native client — view projection.
// Renders `model` into the DOM after every update. No logic here — it is a
// pure function of state (mirroring how the TUI renders its model).

import { model, filteredSessions, isBashDraft, APPROVAL_CHOICES } from './app.js';

const $ = (sel) => document.querySelector(sel);

const els = {
  connection: $('#connection-badge'),
  busy: $('#busy-badge'),
  phase: $('#phase-badge'),
  status: $('#status-msg'),
  transcript: $('#transcript'),
  input: $('#input'),
  promptToken: $('#prompt-token'),
  queuedCount: $('#queued-count'),
  btnSteer: $('#btn-steer'),
  btnQueue: $('#btn-queue'),
  btnCancel: $('#btn-cancel'),
  btnSend: $('#btn-send'),
  btnSessions: $('#btn-sessions'),
  btnRefresh: $('#btn-refresh'),
  hint: $('#hint'),
  footerRight: $('#footer-right'),
  completion: $('#completion'),
  dialogRoot: $('#dialog-root'),
};

// Entry kinds that render as muted, small text.
const MUTED_KINDS = new Set(['thinking', 'status', 'compaction', 'step_summary', 'skill_activation', 'plugin_command', 'cron']);

export function render() {
  renderHeader();
  renderTranscript();
  renderComposer();
  renderCompletion();
  renderDialogs();
}

function renderHeader() {
  els.connection.textContent =
    model.connection === 'connected' ? 'connected' : model.connection === 'failed' ? 'failed' : 'connecting…';
  els.connection.className = `badge ${
    model.connection === 'connected' ? 'badge-secondary' : model.connection === 'failed' ? 'badge-primary' : 'badge-secondary'
  }`;

  els.busy.classList.toggle('hidden', !model.busy);
  els.phase.classList.toggle('hidden', !model.busy || model.phase === 'idle');
  if (model.busy && model.phase !== 'idle') els.phase.textContent = model.phase;
  els.status.textContent = model.statusMsg;
}

function renderTranscript() {
  const root = els.transcript;
  root.textContent = '';
  if (model.entries.length === 0) {
    const empty = document.createElement('div');
    empty.className = 'entry';
    empty.innerHTML = '<div class="body muted">No messages yet. Send a message to start.</div>';
    root.appendChild(empty);
    return;
  }
  for (const e of model.entries) {
    if (e.folded) continue;
    const row = document.createElement('div');
    row.className = 'entry';
    if (e.kind === 'user') {
      row.innerHTML = `<span class="role role-user">you</span><div class="body"></div>`;
      row.querySelector('.body').textContent = e.text;
    } else if (e.kind === 'assistant') {
      row.innerHTML = `<span class="role role-dimi">dimi</span><div class="body"></div>`;
      row.querySelector('.body').textContent = e.text;
    } else if (e.kind === 'tool') {
      row.className = 'entry clickable';
      row.innerHTML = `<span class="role role-tool">tool</span><div class="body tool"><span class="tool-name"></span></div>`;
      row.querySelector('.tool-name').textContent = e.toolName ?? '';
      row.addEventListener('click', () => {
        window.dispatchEvent(new CustomEvent('dimi:msg', { detail: { type: 'tools_expand_toggle', toolCallId: e.toolCallId } }));
      });
    } else if (e.kind === 'thinking') {
      row.innerHTML = `<span class="role role-thinking">thinking</span><div class="body thinking"></div>`;
      row.querySelector('.body').textContent = e.text;
    } else {
      row.innerHTML = `<span class="role role-status"></span><div class="body muted"></div>`;
      row.querySelector('.body').textContent = e.text;
    }
    root.appendChild(row);
  }
  root.scrollTop = root.scrollHeight;
}

function renderComposer() {
  const bash = isBashDraft(model.draft);
  els.promptToken.textContent = bash ? '!' : '>';
  els.promptToken.style.color = bash ? 'var(--info)' : 'var(--accent)';

  // Only touch the textarea value when it differs (keeps the native IME
  // composition and caret undisturbed).
  if (els.input.value !== model.draft) els.input.value = model.draft;

  const canSend = model.draft.trim().length > 0 && !!model.currentSessionId;
  els.btnSend.disabled = !canSend;
  els.btnSend.classList.toggle('hidden', model.draft.trim().length === 0);

  const busyActions = model.busy;
  els.btnSteer.classList.toggle('hidden', !busyActions);
  els.btnQueue.classList.toggle('hidden', !busyActions);
  els.btnCancel.classList.toggle('hidden', !busyActions);
  els.btnSteer.classList.toggle('btn-selected', model.busyInputMode === 'steer');
  els.btnQueue.classList.toggle('btn-selected', model.busyInputMode === 'queue');
  els.btnSteer.textContent = 'steer';
  els.btnQueue.textContent = 'queue';

  els.queuedCount.classList.toggle('hidden', model.queued.length === 0);
  if (model.queued.length > 0) els.queuedCount.textContent = `${model.queued.length} queued`;

  // Footer hint (TUI queue-pane adaptive hint).
  let hint = '';
  if (model.phase === 'compacting' && !model.busy) {
    hint = '↑ to edit · will send after compaction';
  } else if (model.queued.length > 0 && model.busyInputMode === 'steer') {
    hint = '↑ to edit · enter steers · ctrl-s flushes queue';
  } else if (model.queued.some((q) => q.mode !== 'bash') && model.phase === 'streaming') {
    hint = '↑ to edit · ctrl-s to steer immediately';
  } else if (model.queued.length > 0) {
    hint = '↑ to edit · will send after current task';
  }
  els.hint.textContent = hint;

  els.footerRight.textContent = model.currentSessionId
    ? `${model.entryCount} messages · ${model.displayMode}`
    : '';
}

function renderCompletion() {
  const root = els.completion;
  const open = model.completionOpen || model.atMentionOpen;
  if (!open || model.completionItems.length === 0) {
    root.classList.add('hidden');
    root.textContent = '';
    return;
  }
  root.classList.remove('hidden');
  root.textContent = '';
  model.completionItems.forEach((item, i) => {
    const div = document.createElement('div');
    div.className = 'completion-item' + (i === model.completionSelected ? ' selected' : '');
    div.innerHTML = `<span class="value"></span><span class="desc"></span>`;
    div.querySelector('.value').textContent = item.label;
    if (item.description) div.querySelector('.desc').textContent = item.description;
    div.addEventListener('mousedown', (evt) => {
      evt.preventDefault();
      window.dispatchEvent(new CustomEvent('dimi:msg', { detail: { type: 'completion_accept' } }));
    });
    root.appendChild(div);
  });
  const sel = root.querySelector('.selected');
  if (sel) sel.scrollIntoView({ block: 'nearest' });
}

function renderDialogs() {
  const root = els.dialogRoot;
  root.textContent = '';

  if (model.pickerOpen) renderSessionPicker(root);
  if (model.settingsDialogOpen) renderSettingsDialog(root);
  if (model.currentApproval) renderApproval(root);
  if (model.currentQuestion) renderQuestion(root);
}

function dialog(title, bodyNode, footerNodes) {
  const backdrop = document.createElement('div');
  backdrop.className = 'dialog-backdrop';
  const dlg = document.createElement('div');
  dlg.className = 'dialog';
  const titleEl = document.createElement('div');
  titleEl.className = 'dialog-title';
  titleEl.textContent = title;
  dlg.appendChild(titleEl);
  const body = document.createElement('div');
  body.className = 'dialog-body';
  body.appendChild(bodyNode);
  dlg.appendChild(body);
  if (footerNodes && footerNodes.length > 0) {
    const foot = document.createElement('div');
    foot.className = 'dialog-footer';
    for (const f of footerNodes) foot.appendChild(f);
    dlg.appendChild(foot);
  }
  backdrop.appendChild(dlg);
  return backdrop;
}

function renderSessionPicker(root) {
  const list = filteredSessions(model);
  const body = document.createElement('div');
  body.style.display = 'flex';
  body.style.flexDirection = 'column';
  body.style.gap = '8px';
  body.style.minHeight = '300px';

  const search = document.createElement('input');
  search.className = 'search-input';
  search.placeholder = 'Search sessions…';
  search.value = model.pickerQuery;
  search.addEventListener('input', () => {
    window.dispatchEvent(new CustomEvent('dimi:msg', { detail: { type: 'picker_search', query: search.value } }));
  });
  search.addEventListener('keydown', (evt) => {
    // Picker keys (TUI session-picker.ts): ↑/↓ move, Enter selects,
    // Esc clears query first then closes.
    if (evt.key === 'ArrowDown') { evt.preventDefault(); window.dispatchEvent(new CustomEvent('dimi:msg', { detail: { type: 'picker_move', delta: 1 } })); }
    else if (evt.key === 'ArrowUp') { evt.preventDefault(); window.dispatchEvent(new CustomEvent('dimi:msg', { detail: { type: 'picker_move', delta: -1 } })); }
    else if (evt.key === 'Enter') { evt.preventDefault(); window.dispatchEvent(new CustomEvent('dimi:msg', { detail: { type: 'picker_select' } })); }
    else if (evt.key === 'Escape') { evt.preventDefault(); window.dispatchEvent(new CustomEvent('dimi:msg', { detail: { type: 'escape' } })); }
  });
  body.appendChild(search);

  const listEl = document.createElement('div');
  listEl.style.overflowY = 'auto';
  listEl.style.maxHeight = '320px';
  if (list.length === 0) {
    const empty = document.createElement('div');
    empty.className = 'list-item';
    empty.textContent = model.sessionsLoading ? 'Loading sessions…' : 'No sessions found.';
    listEl.appendChild(empty);
  } else {
    list.forEach((s, i) => {
      const item = document.createElement('div');
      item.className = 'list-item' + (i === model.pickerSelectedIndex ? ' selected' : '');
      const title = document.createElement('div');
      title.className = 'title';
      title.textContent = s.title || '(untitled)';
      const sub = document.createElement('div');
      sub.className = 'sub';
      sub.textContent = `${s.id} · ${s.cwd ?? ''}`;
      item.appendChild(title);
      item.appendChild(sub);
      item.addEventListener('mousedown', (evt) => {
        evt.preventDefault();
        window.dispatchEvent(new CustomEvent('dimi:msg', { detail: { type: 'session_selected', id: s.id } }));
      });
      listEl.appendChild(item);
    });
  }
  body.appendChild(listEl);

  const hint = document.createElement('div');
  hint.className = 'sub';
  hint.textContent = '↑↓ navigate · Enter select · Esc cancel';
  body.appendChild(hint);

  const btnClose = document.createElement('button');
  btnClose.className = 'btn btn-ghost';
  btnClose.textContent = 'Close';
  btnClose.addEventListener('click', () => window.dispatchEvent(new CustomEvent('dimi:msg', { detail: { type: 'picker_close' } })));
  root.appendChild(dialog('Sessions', body, [btnClose]));
}

function renderSettingsDialog(root) {
  const body = document.createElement('div');
  body.textContent = 'Settings — coming in next pass.';
  const btn = document.createElement('button');
  btn.className = 'btn btn-ghost';
  btn.textContent = 'Close';
  btn.addEventListener('click', () => window.dispatchEvent(new CustomEvent('dimi:msg', { detail: { type: 'settings_close' } })));
  root.appendChild(dialog('Settings', body, [btn]));
}

function renderApproval(root) {
  const a = model.currentApproval;
  const body = document.createElement('div');
  body.style.display = 'flex';
  body.style.flexDirection = 'column';
  body.style.gap = '6px';
  body.style.maxWidth = '560px';

  const title = document.createElement('div');
  title.className = 'title';
  title.textContent = a.toolName || 'Approval required';
  body.appendChild(title);

  const action = document.createElement('div');
  action.className = 'body';
  action.style.fontFamily = 'monospace';
  action.textContent = a.action || '';
  body.appendChild(action);

  if (a.command) {
    const cmd = document.createElement('div');
    cmd.className = 'body';
    cmd.style.fontFamily = 'monospace';
    cmd.style.color = 'var(--text-muted)';
    cmd.textContent = a.command;
    body.appendChild(cmd);
  }

  const options = document.createElement('div');
  options.style.display = 'flex';
  options.style.flexDirection = 'column';
  options.style.gap = '4px';
  APPROVAL_CHOICES.forEach((opt, i) => {
    const o = document.createElement('div');
    o.className = 'list-item' + (i === model.approvalSelectedIndex ? ' selected' : '');
    o.textContent = opt.label;
    o.addEventListener('mousedown', (evt) => {
      evt.preventDefault();
      window.dispatchEvent(new CustomEvent('dimi:msg', { detail: { type: 'approval_select', index: i } }));
      window.dispatchEvent(new CustomEvent('dimi:msg', { detail: { type: 'approval_confirm' } }));
    });
    options.appendChild(o);
  });
  body.appendChild(options);

  const hint = document.createElement('div');
  hint.className = 'sub';
  hint.textContent = model.approvalPreview
    ? 'previewing tool input (Ctrl+E to hide)'
    : '↑↓ navigate · Enter confirm · 1-4 select · Esc reject · Ctrl+E preview';
  body.appendChild(hint);

  // Feedback input for "Reject with feedback…" (TUI approval-panel.ts:250-259).
  if (model.approvalSelectedIndex === 3) {
    const fb = document.createElement('input');
    fb.className = 'search-input';
    fb.placeholder = 'Feedback…';
    fb.value = model.approvalFeedbackText;
    fb.addEventListener('input', () => {
      model.approvalFeedbackText = fb.value;
    });
    body.appendChild(fb);
  }

  root.appendChild(dialog('Approval', body, []));
}

function renderQuestion(root) {
  const q = model.currentQuestion;
  const body = document.createElement('div');
  body.style.display = 'flex';
  body.style.flexDirection = 'column';
  body.style.gap = '6px';
  body.style.maxWidth = '520px';

  const title = document.createElement('div');
  title.className = 'title';
  title.textContent = q.question ?? 'Question';
  body.appendChild(title);

  const options = document.createElement('div');
  options.style.display = 'flex';
  options.style.flexDirection = 'column';
  options.style.gap = '4px';
  (q.options ?? []).forEach((opt, i) => {
    const o = document.createElement('div');
    o.className = 'list-item' + (i === model.questionSelectedIndex ? ' selected' : '');
    const marker = opt.selected ? (q.kind === 'multi' || q.kind === 'multi_with_other' ? '✓ ' : '● ') : (q.kind === 'multi' || q.kind === 'multi_with_other' ? '○ ' : '○ ');
    o.textContent = marker + opt.label;
    o.addEventListener('mousedown', (evt) => {
      evt.preventDefault();
      if (q.kind === 'multi' || q.kind === 'multi_with_other') {
        window.dispatchEvent(new CustomEvent('dimi:msg', { detail: { type: 'question_toggle', index: i } }));
      } else {
        window.dispatchEvent(new CustomEvent('dimi:msg', { detail: { type: 'question_select', index: i } }));
      }
    });
    options.appendChild(o);
  });
  body.appendChild(options);

  const btn = document.createElement('button');
  btn.className = 'btn btn-primary';
  btn.textContent = 'Submit';
  btn.addEventListener('click', () => window.dispatchEvent(new CustomEvent('dimi:msg', { detail: { type: 'question_confirm' } })));
  root.appendChild(dialog('Question', body, [btn]));
}

export { els };
