// Dimi native client — view projection.
// Renders `model` into the DOM after every update. No logic here — it is a
// pure function of state (mirroring how the TUI renders its model).

import { model, filteredSessions, isBashDraft, APPROVAL_CHOICES, slashCommands } from './app.js';

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
      // TUI user message: ✨ bullet + accent-colored text, no bubble.
      const role = document.createElement('span');
      role.className = 'role role-user';
      role.textContent = '✨';
      const body = document.createElement('div');
      body.className = 'body';
      body.style.color = 'var(--accent)';
      body.textContent = e.text;
      row.appendChild(role);
      row.appendChild(body);
    } else if (e.kind === 'assistant') {
      // TUI assistant message: plain label + text, no bubble.
      const role = document.createElement('span');
      role.className = 'role role-dimi';
      role.textContent = 'dimi';
      const body = document.createElement('div');
      body.className = 'body';
      body.textContent = e.text;
      row.appendChild(role);
      row.appendChild(body);
    } else if (e.kind === 'tool') {
      row.className = 'entry clickable';
      row.innerHTML = `<span class="role role-tool">tool</span><div class="body tool"><span class="tool-name"></span><div class="tool-output"></div></div>`;
      row.querySelector('.tool-name').textContent = `${e.toolName ?? ''}${e.expanded ? ' ▾' : ' ▸'}`;
      const out = row.querySelector('.tool-output');
      if (e.expanded && e.text) {
        out.style.display = 'block';
        out.style.marginTop = '4px';
        out.style.fontFamily = 'monospace';
        out.style.whiteSpace = 'pre-wrap';
        out.style.wordBreak = 'break-word';
        out.style.maxHeight = '200px';
        out.style.overflow = 'auto';
        out.style.color = 'var(--text-muted)';
        out.textContent = e.text;
      } else {
        out.style.display = 'none';
      }
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
  // Send button stays visible (TUI always shows submit); only disabled when
  // there is nothing to send.
  els.btnSend.classList.remove('hidden');

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
    div.innerHTML = `<span class="pointer"></span><span class="value"></span><span class="desc"></span>`;
    // TUI SELECT_POINTER: the highlighted row gets a ❯ prefix.
    div.querySelector('.pointer').textContent = i === model.completionSelected ? '❯ ' : '  ';
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
  if (model.helpDialogOpen) renderHelpDialog(root);
  if (model.btwOpen) renderBtw(root);
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
    // Esc clears query first then closes, Ctrl+A toggles scope.
    if (evt.key === 'ArrowDown') { evt.preventDefault(); window.dispatchEvent(new CustomEvent('dimi:msg', { detail: { type: 'picker_move', delta: 1 } })); }
    else if (evt.key === 'ArrowUp') { evt.preventDefault(); window.dispatchEvent(new CustomEvent('dimi:msg', { detail: { type: 'picker_move', delta: -1 } })); }
    else if (evt.key === 'Enter') { evt.preventDefault(); window.dispatchEvent(new CustomEvent('dimi:msg', { detail: { type: 'picker_select' } })); }
    else if (evt.key === 'Escape') { evt.preventDefault(); window.dispatchEvent(new CustomEvent('dimi:msg', { detail: { type: 'escape' } })); }
    else if ((evt.metaKey || evt.ctrlKey) && evt.key.toLowerCase() === 'a') {
      evt.preventDefault();
      window.dispatchEvent(new CustomEvent('dimi:msg', { detail: { type: 'picker_scope', scope: model.pickerScope === 'cwd' ? 'all' : 'cwd' } }));
    }
  });
  body.appendChild(search);

  const listEl = document.createElement('div');
  listEl.style.overflowY = 'auto';
  listEl.style.maxHeight = '320px';
  // TUI session-picker: near the bottom, load the next page (pageSize=50).
  listEl.addEventListener('scroll', () => {
    if (listEl.scrollTop + listEl.clientHeight >= listEl.scrollHeight - 40) {
      window.dispatchEvent(new CustomEvent('dimi:msg', { detail: { type: 'picker_load_more' } }));
    }
  });
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
      const rel = relativeTime(s.updated_at);
      const cwd = s.metadata?.cwd ?? s.cwd ?? '';
      const last = s.last_prompt ? ` · "${truncate(s.last_prompt, 40)}"` : '';
      sub.textContent = `${s.id} · ${rel}${cwd ? ` · ${cwd}` : ''}${last}`;
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

function renderHelpDialog(root) {
  const body = document.createElement('div');
  body.style.maxWidth = '560px';
  body.style.maxHeight = '420px';
  body.style.overflowY = 'auto';
  // The command table lives in app.js — import at module top instead.
  const rows = slashCommands.map((c) => {
    const row = document.createElement('div');
    row.className = 'list-item';
    row.style.padding = '4px 8px';
    const line = document.createElement('div');
    line.innerHTML = `<span class="tool-name">/${c.name}</span>${c.hint ? ` <span class="sub">${c.hint}</span>` : ''} <span class="sub">— ${c.desc}</span>`;
    row.appendChild(line);
    return row;
  });
  for (const r of rows) body.appendChild(r);
  const btn = document.createElement('button');
  btn.className = 'btn btn-ghost';
  btn.textContent = 'Close';
  btn.addEventListener('click', () => { model.helpDialogOpen = false; render(); });
  root.appendChild(dialog('Help', body, [btn]));
}

function renderSettingsDialog(root) {
  const body = document.createElement('div');
  body.style.display = 'flex';
  body.style.flexDirection = 'column';
  body.style.gap = '10px';
  body.style.minWidth = '460px';

  // Model selector (TUI model-selector): loads /models on open.
  const modelRow = field('Default model');
  const modelSel = document.createElement('select');
  modelSel.className = 'search-input';
  modelSel.innerHTML = '<option>loading…</option>';
  modelSel.addEventListener('change', () => {
    if (!modelSel.value || modelSel.value === 'loading') return;
    window.dispatchEvent(new CustomEvent('dimi:msg', { detail: { type: 'settings_set_model', ref: modelSel.value } }));
  });
  modelRow.appendChild(modelSel);
  body.appendChild(modelRow);
  loadModelsInto(modelSel);

  // Permission mode selector.
  const permRow = field('Permission mode');
  const permSel = document.createElement('select');
  permSel.className = 'search-input';
  for (const m of ['manual', 'auto', 'yolo']) {
    const o = document.createElement('option');
    o.value = m; o.textContent = m;
    permSel.appendChild(o);
  }
  permSel.addEventListener('change', () => {
    window.dispatchEvent(new CustomEvent('dimi:msg', { detail: { type: 'settings_set_permission', mode: permSel.value } }));
  });
  permRow.appendChild(permSel);
  body.appendChild(permRow);

  // Plan mode toggle.
  const planRow = field('Plan mode');
  const planBtn = document.createElement('button');
  planBtn.className = 'btn btn-ghost';
  planBtn.textContent = model.planMode ? 'on (toggle)' : 'off (toggle)';
  planBtn.addEventListener('click', () => {
    window.dispatchEvent(new CustomEvent('dimi:msg', { detail: { type: 'plan_mode_toggle' } }));
  });
  planRow.appendChild(planBtn);
  body.appendChild(planRow);

  // Thinking effort.
  const effortRow = field('Thinking effort');
  const effortSel = document.createElement('select');
  effortSel.className = 'search-input';
  for (const e of ['off', 'low', 'medium', 'high']) {
    const o = document.createElement('option');
    o.value = e; o.textContent = e;
    effortSel.appendChild(o);
  }
  effortSel.addEventListener('change', () => {
    window.dispatchEvent(new CustomEvent('dimi:msg', { detail: { type: 'settings_set_effort', effort: effortSel.value } }));
  });
  effortRow.appendChild(effortSel);
  body.appendChild(effortRow);

  // Theme.
  const themeRow = field('Theme');
  const themeSel = document.createElement('select');
  themeSel.className = 'search-input';
  for (const t of ['auto', 'dark', 'light']) {
    const o = document.createElement('option');
    o.value = t; o.textContent = t;
    themeSel.appendChild(o);
  }
  themeSel.value = model.theme;
  themeSel.addEventListener('change', () => {
    window.dispatchEvent(new CustomEvent('dimi:msg', { detail: { type: 'settings_set_theme', theme: themeSel.value } }));
  });
  themeRow.appendChild(themeSel);
  body.appendChild(themeRow);

  const btn = document.createElement('button');
  btn.className = 'btn btn-ghost';
  btn.textContent = 'Close';
  btn.addEventListener('click', () => window.dispatchEvent(new CustomEvent('dimi:msg', { detail: { type: 'settings_close' } })));
  root.appendChild(dialog('Settings', body, [btn]));
}

function field(label) {
  const row = document.createElement('div');
  row.style.display = 'flex';
  row.style.alignItems = 'center';
  row.style.gap = '8px';
  const lab = document.createElement('span');
  lab.className = 'sub';
  lab.style.minWidth = '110px';
  lab.textContent = label;
  row.appendChild(lab);
  return row;
}

async function loadModelsInto(sel) {
  try {
    const data = await window.dimi.request({ method: 'GET', url: '/api/v1/models' });
    const items = data?.json?.data?.items ?? [];
    sel.innerHTML = '';
    if (items.length === 0) {
      const o = document.createElement('option');
      o.value = ''; o.textContent = 'no models';
      sel.appendChild(o);
      return;
    }
    for (const m of items) {
      const o = document.createElement('option');
      // Model id is the provider/model reference.
      o.value = `${m.provider}/${m.model}`;
      o.textContent = `${m.display_name ?? m.model} (${m.provider}/${m.model})`;
      sel.appendChild(o);
    }
  } catch {
    sel.innerHTML = '<option>failed to load models</option>';
  }
}

function renderBtw(root) {
  const body = document.createElement('div');
  body.style.display = 'flex';
  body.style.flexDirection = 'column';
  body.style.gap = '8px';
  body.style.minWidth = '440px';

  const chat = document.createElement('div');
  chat.style.display = 'flex';
  chat.style.flexDirection = 'column';
  chat.style.gap = '6px';
  chat.style.maxHeight = '260px';
  chat.style.overflowY = 'auto';

  if (model.btwPrompt) {
    const p = document.createElement('div');
    p.className = 'list-item';
    p.innerHTML = `<span class="role role-user">you</span>`;
    const pt = document.createElement('div');
    pt.className = 'body';
    pt.textContent = model.btwPrompt;
    p.appendChild(pt);
    chat.appendChild(p);
  }
  if (model.btwAnswer) {
    const a = document.createElement('div');
    a.className = 'list-item';
    const at = document.createElement('div');
    at.className = 'body';
    at.textContent = model.btwAnswer;
    a.appendChild(at);
    chat.appendChild(a);
  } else if (model.btwBusy) {
    const b = document.createElement('div');
    b.className = 'sub';
    b.textContent = '…';
    chat.appendChild(b);
  }
  body.appendChild(chat);

  const input = document.createElement('input');
  input.className = 'search-input';
  input.placeholder = 'Ask by the way…';
  input.value = model.btwDraft;
  input.addEventListener('keydown', (evt) => {
    if (evt.key === 'Enter' && input.value.trim()) {
      evt.preventDefault();
      window.dispatchEvent(new CustomEvent('dimi:msg', { detail: { type: 'btw_send', text: input.value } }));
      input.value = '';
    } else if (evt.key === 'Escape') {
      evt.preventDefault();
      window.dispatchEvent(new CustomEvent('dimi:msg', { detail: { type: 'escape' } }));
    }
  });
  body.appendChild(input);

  const hint = document.createElement('div');
  hint.className = 'sub';
  hint.textContent = 'Enter to ask · Esc to close';
  body.appendChild(hint);

  root.appendChild(dialog('BTW', body, []));
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
    const cmd = document.createElement('pre');
    cmd.className = 'body';
    cmd.style.fontFamily = 'monospace';
    cmd.style.color = 'var(--text-muted)';
    cmd.style.whiteSpace = 'pre-wrap';
    cmd.style.wordBreak = 'break-word';
    cmd.style.maxHeight = model.approvalPreview ? '220px' : '72px';
    cmd.style.overflow = 'auto';
    cmd.style.background = 'var(--bg)';
    cmd.style.padding = '6px 8px';
    cmd.style.borderRadius = '6px';
    cmd.style.border = '1px solid var(--border)';
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
    fb.addEventListener('keydown', (evt) => {
      if (evt.key === 'Enter') {
        evt.preventDefault();
        // Submit the rejection with feedback.
        model.approvalFeedbackMode = false;
        window.dispatchEvent(new CustomEvent('dimi:msg', { detail: { type: 'approval_confirm' } }));
      } else if (evt.key === 'Escape') {
        evt.preventDefault();
        model.approvalFeedbackMode = false;
        model.approvalFeedbackText = '';
        render();
      }
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
  body.style.maxWidth = '560px';

  // Tabs: one per question + Submit (TUI question-dialog.ts:604-627).
  const total = (q.allQuestions?.length ?? 1) + 1;
  const cur = q.questionTabIndex ?? 0;
  const tabs = document.createElement('div');
  tabs.style.display = 'flex';
  tabs.style.gap = '4px';
  tabs.style.flexWrap = 'wrap';
  (q.allQuestions ?? [q]).forEach((qq, i) => {
    const t = document.createElement('span');
    t.className = 'badge' + (i === cur ? ' badge-primary' : ' badge-outline');
    t.textContent = `Q${i + 1}${hasAnswer(qq) ? ' ✓' : ''}`;
    t.style.cursor = 'pointer';
    t.addEventListener('mousedown', (evt) => {
      evt.preventDefault();
      model.currentQuestion.questionTabIndex = i;
      // Sync the tab's question into the top-level fields the view reads.
      const q = model.currentQuestion;
      const qq = q.allQuestions?.[i];
      if (qq) {
        q.itemId = qq.itemId;
        q.question = qq.question;
        q.kind = qq.kind;
        q.options = qq.options;
        q.allowOther = qq.allowOther;
        q.otherLabel = qq.otherLabel;
        q.otherText = qq.otherText;
      }
      model.questionSelectedIndex = 0;
      render();
    });
    tabs.appendChild(t);
  });
  const submitTab = document.createElement('span');
  submitTab.className = 'badge' + (cur === total - 1 ? ' badge-primary' : ' badge-outline');
  submitTab.textContent = 'Submit';
  submitTab.style.cursor = 'pointer';
  submitTab.addEventListener('mousedown', (evt) => {
    evt.preventDefault();
    model.currentQuestion.questionTabIndex = total - 1;
    render();
  });
  tabs.appendChild(submitTab);
  body.appendChild(tabs);

  // Submit tab: review answers (TUI review page).
  if (cur === total - 1) {
    const review = document.createElement('div');
    review.style.display = 'flex';
    review.style.flexDirection = 'column';
    review.style.gap = '4px';
    for (const qq of q.allQuestions ?? [q]) {
      const row = document.createElement('div');
      const answered = hasAnswer(qq);
      row.className = 'list-item' + (answered ? '' : '');
      row.textContent = `${answered ? '✓' : '○'} ${qq.question}${answered ? '' : ' — Not answered'}`;
      review.appendChild(row);
    }
    body.appendChild(review);
    const btn = document.createElement('button');
    btn.className = 'btn btn-primary';
    btn.textContent = 'Submit';
    btn.addEventListener('click', () => window.dispatchEvent(new CustomEvent('dimi:msg', { detail: { type: 'question_confirm' } })));
    root.appendChild(dialog('Question', body, [btn]));
    return;
  }

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
    const multi = q.kind === 'multi' || q.kind === 'multi_with_other';
    const marker = opt.selected ? (multi ? '✓ ' : '● ') : (multi ? '○ ' : '○ ');
    o.textContent = marker + opt.label;
    o.addEventListener('mousedown', (evt) => {
      evt.preventDefault();
      if (multi) {
        window.dispatchEvent(new CustomEvent('dimi:msg', { detail: { type: 'question_toggle', index: i } }));
      } else {
        window.dispatchEvent(new CustomEvent('dimi:msg', { detail: { type: 'question_select', index: i } }));
        window.dispatchEvent(new CustomEvent('dimi:msg', { detail: { type: 'question_confirm' } }));
      }
    });
    options.appendChild(o);
  });
  body.appendChild(options);

  // Other input (TUI question-dialog.ts:696-712).
  if (q.allowOther) {
    const other = document.createElement('input');
    other.className = 'search-input';
    other.placeholder = q.otherLabel || 'Other…';
    other.value = q.otherText ?? model.questionOtherText ?? '';
    other.addEventListener('input', () => {
      window.dispatchEvent(new CustomEvent('dimi:msg', { detail: { type: 'question_other', text: other.value } }));
    });
    body.appendChild(other);
  }

  const hint = document.createElement('div');
  hint.className = 'sub';
  hint.textContent = '←/→ tabs · 1-9 select · space toggle · Enter confirm';
  body.appendChild(hint);

  const btn = document.createElement('button');
  btn.className = 'btn btn-primary';
  btn.textContent = 'Next';
  btn.addEventListener('click', () => window.dispatchEvent(new CustomEvent('dimi:msg', { detail: { type: 'question_tab', delta: 1 } })));
  root.appendChild(dialog('Question', body, [btn]));
}

function hasAnswer(qq) {
  return (qq.options ?? []).some((o) => o.selected) || (qq.otherText && qq.otherText.trim().length > 0);
}

function relativeTime(iso) {
  if (!iso) return '';
  const t = new Date(iso).getTime();
  if (Number.isNaN(t)) return '';
  const s = Math.max(0, Math.floor((Date.now() - t) / 1000));
  if (s < 60) return 'just now';
  if (s < 3600) return `${Math.floor(s / 60)}m ago`;
  if (s < 86400) return `${Math.floor(s / 3600)}h ago`;
  return `${Math.floor(s / 86400)}d ago`;
}

function truncate(text, n) {
  if (!text) return '';
  return text.length > n ? text.slice(0, n) + '…' : text;
}

export { els };
