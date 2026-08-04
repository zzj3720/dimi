// Dimi native client — transport & effects (migrated from main.js).
// All server communication goes through window.dimi (Electron main bridge).
// Vue reactivity replaces the old imperative render() calls.

import { state, update, Msg, saveHistory, isBashDraft, findSlashCommand, APPROVAL_CHOICES } from './store';
import type { Msg as MsgType, State, Entry, SessionSummary } from './store';

// ------------------------------------------------------------------ bridge

function dimi(): Window['dimi'] {
  return window.dimi as Window['dimi'];
}

export async function api(method: string, path: string, body?: unknown): Promise<any> {
  const res = await dimi().request({ method, url: path, body });
  if (!res.ok) {
    throw new Error(`${method} ${path} -> ${res.status}: ${res.text?.slice(0, 200) ?? ''}`);
  }
  return res.json;
}

// ------------------------------------------------------------------ dispatch

// Update the reactive state, then run side effects (network/focus).
export function dispatch(msg: MsgType): void {
  update(state, msg);
  afterDispatch(msg);
}

function afterDispatch(msg: MsgType): void {
  switch (msg.type) {
    case 'session_selected':
      rememberSession(msg.id as string);
      void connectSession(msg.id as string);
      break;
    case 'picker_select': {
      if (state.currentSessionId) {
        rememberSession(state.currentSessionId);
        void connectSession(state.currentSessionId);
      }
      break;
    }
    case 'submit':
      submitDraft();
      break;
    case 'steer':
      doSteer();
      break;
    case 'cancel':
      if (state.cancelStreamRequested) {
        state.cancelStreamRequested = false;
        doCancel();
      }
      break;
    case 'approval_confirm':
      submitApproval();
      break;
    case 'approval_reject':
      if (state.approvalRejectRequested) {
        state.approvalRejectRequested = false;
        rejectApproval();
      }
      break;
    case 'question_dismiss':
      if (state.questionDismissRequested) {
        state.questionDismissRequested = false;
        dismissQuestion();
      }
      break;
    case 'question_confirm':
      submitQuestion();
      break;
    case 'escape':
      if (state.undoRequested) {
        state.undoRequested = false;
        runUndo(1);
      }
      if (state.approvalRejectRequested) {
        state.approvalRejectRequested = false;
        rejectApproval();
      }
      if (state.questionDismissRequested) {
        state.questionDismissRequested = false;
        dismissQuestion();
      }
      if (state.cancelStreamRequested) {
        state.cancelStreamRequested = false;
        doCancel();
      }
      break;
  }
}

// ------------------------------------------------------------------ sessions

export async function loadSessions(): Promise<void> {
  state.sessionsLoading = true;
  try {
    const data = await api('GET', '/api/v1/sessions?page_size=50');
    const items = (data?.data?.items ?? []) as SessionSummary[];
    state.sessions = items;
    state.sessionsHasMore = !!data?.data?.has_more;
    state.sessionsLoading = false;
    dispatch(Msg.SessionsLoaded(items));
    if (!state.currentSessionId && items.length > 0) {
      const last = readLastSessionId();
      const target = items.find((s) => s.id === last) ?? items[0];
      dispatch(Msg.SessionSelected(target.id));
    } else if (!state.currentSessionId && items.length === 0) {
      const id = await createSession();
      if (id) dispatch(Msg.SessionSelected(id));
    }
  } catch (e) {
    state.sessionsLoading = false;
    state.sessionsError = String(e);
    state.statusMsg = `failed to load sessions: ${(e as Error).message}`;
  }
}

export async function loadMoreSessions(): Promise<void> {
  if (state.sessionsLoading || !state.sessionsHasMore) return;
  state.sessionsLoading = true;
  const last = state.sessions[state.sessions.length - 1];
  try {
    const data = await api('GET', `/api/v1/sessions?page_size=50&before_id=${encodeURIComponent(last?.id ?? '')}`);
    const items = (data?.data?.items ?? []) as SessionSummary[];
    state.sessions = state.sessions.concat(items);
    state.sessionsHasMore = !!data?.data?.has_more;
    state.sessionsLoading = false;
    dispatch(Msg.SessionsLoaded(state.sessions));
  } catch (e) {
    state.sessionsLoading = false;
    state.statusMsg = `load more failed: ${(e as Error).message}`;
  }
}

export async function connectSession(sessionId: string): Promise<void> {
  try {
    const data = await api('GET', `/api/v1/sessions/${sessionId}/messages?page_size=100`);
    const msgs = (data?.data?.items ?? []) as Record<string, unknown>[];
    state.entries = msgsToEntries(msgs);
    state.entryCount = state.entries.length;
  } catch (e) {
    state.statusMsg = `failed to load messages: ${(e as Error).message}`;
  }
  subscribeSse(sessionId);
  void fetchStatus(sessionId);

  const s = state.sessions.find((x) => x.id === sessionId);
  state.currentCwd = s?.metadata?.cwd ?? s?.cwd ?? '';
  try {
    const prof = await api('GET', `/api/v1/sessions/${sessionId}`);
    const m = prof?.data?.agent_config?.model;
    if (m) state.modelName = m;
  } catch {
    /* non-fatal */
  }
}

export async function fetchStatus(sessionId: string): Promise<void> {
  try {
    const data = await api('GET', `/api/v1/sessions/${sessionId}/status`);
    const st = data?.data as Record<string, unknown> | undefined;
    if (st) {
      state.busy = !!st.busy;
      state.phase = st.busy ? 'streaming' : 'idle';
      if (st.model) state.modelName = String(st.model);
      if (st.permission) state.permissionMode = st.permission as State['permissionMode'];
      if (typeof st.plan_mode === 'boolean') state.planMode = st.plan_mode;
      if (typeof st.context_tokens === 'number' && typeof st.max_context_tokens === 'number') {
        const max = st.max_context_tokens;
        const pct = max > 0 ? Math.round((st.context_tokens / max) * 100) : 0;
        state.footerContext = `context: ${pct}% (${fmtK(st.context_tokens)}/${fmtK(max)})`;
      }
    }
  } catch {
    /* non-fatal */
  }
}

function fmtK(n: number): string {
  if (n >= 1024) {
    const v = n / 1024;
    return (v >= 10 ? String(Math.round(v)) : v.toFixed(1)) + 'k';
  }
  return String(n);
}

export function subscribeSse(sessionId: string): void {
  if (state.sseUnsubscribe) {
    state.sseUnsubscribe();
    state.sseUnsubscribe = null;
  }
  state.sseUnsubscribe = dimi().subscribeEvents(`/api/v1/sessions/${sessionId}/events`, (evt) => {
    const e = evt as { payload?: Record<string, unknown>; type?: string };
    const p = (e.payload ?? e) as Record<string, unknown>;
    const type = (p.type as string) ?? e.type;
    if (type === 'turn.ended' || type === 'prompt.completed' || type === 'prompt.steered') {
      void refreshPromptQueue();
    }
    dispatch(Msg.SseEvent(evt));
    if (type === 'turn.ended' && !state.busy) {
      setTimeout(() => {
        if (!state.busy) drainQueuedBash();
      }, 0);
    }
  });
}

// ------------------------------------------------------------------ submit

export function submitDraft(): void {
  const draft = state.draft;
  if (draft.trim().length === 0) return;
  if (!state.currentSessionId) {
    state.statusMsg = 'select a session first';
    return;
  }

  if (state.inputHistory[state.inputHistory.length - 1] !== draft) {
    state.inputHistory.push(draft);
  }
  state.historyIndex = -1;
  saveHistory(state);

  if (isBashDraft(draft)) {
    if (state.busy) {
      state.queued.push({ text: draft, mode: 'bash' });
      state.statusMsg = `${state.queued.length} queued`;
    } else {
      runShellCommand(draft);
    }
    state.draft = '';
    return;
  }

  if (draft.startsWith('/')) {
    const resolved = parseSlash(draft);
    if (resolved.known) {
      runSlashCommand(resolved);
      if (!state.lastCommandError) {
        state.draft = '';
      }
      return;
    }
  }

  void sendPrompt(draft);
  state.draft = '';
}

export interface ParsedSlash {
  kind: string;
  name: string;
  args: string;
  command: ReturnType<typeof findSlashCommand>;
  known: boolean;
}

function parseSlash(text: string): ParsedSlash {
  const body = text.slice(1);
  if (body.includes('/') && !body.includes(':')) {
    return { kind: 'path', name: text, args: '', command: null, known: false };
  }
  const m = text.match(/^\/(\S+)\s*([\s\S]*)$/);
  const name = m?.[1] ?? body;
  const args = m?.[2]?.trim() ?? '';
  const cmd = findSlashCommand(name);
  return { kind: 'command', name, args, command: cmd, known: cmd !== null };
}

function runSlashCommand(resolved: ParsedSlash): void {
  state.lastCommandError = '';
  const cmd = resolved.command;
  if (!cmd) return;
  if (cmd.idleOnly && state.busy) {
    state.lastCommandError = 'Cannot run this command while streaming — press Esc or Ctrl-C first.';
    return;
  }
  switch (cmd.name) {
    case 'help':
      state.statusMsg = '';
      state.helpDialogOpen = true;
      break;
    case 'exit':
      window.close();
      break;
    case 'version':
      state.statusMsg = 'Dimi Client 0.1.0';
      break;
    case 'sessions':
      dispatch(Msg.PickerOpen());
      void loadSessions();
      break;
    case 'new': {
      void createSession().then((id) => {
        if (id) dispatch(Msg.SessionSelected(id));
      });
      break;
    }
    case 'theme':
      applyTheme(resolved.args || 'auto');
      break;
    case 'settings':
      dispatch(Msg.SettingsOpen());
      break;
    case 'status': {
      const s = state.sessions.find((x) => x.id === state.currentSessionId);
      state.statusMsg = `session=${state.currentSessionId || '-'} · busy=${state.busy} · phase=${state.phase} · title=${s?.title ?? ''}`;
      break;
    }
    case 'copy': {
      for (let i = state.entries.length - 1; i >= 0; i--) {
        if (state.entries[i].kind === 'assistant') {
          void navigator.clipboard.writeText(state.entries[i].text).then(() => {
            state.statusMsg = 'copied last assistant message';
          });
          break;
        }
      }
      break;
    }
    case 'compact':
      if (!state.currentSessionId) {
        state.statusMsg = 'select a session first';
        return;
      }
      api('POST', `/api/v1/sessions/${state.currentSessionId}:compact`, {
        instruction: resolved.args || undefined,
      })
        .then(() => {
          state.statusMsg = 'compacting…';
        })
        .catch((e) => {
          state.statusMsg = `compact failed: ${(e as Error).message}`;
        });
      break;
    case 'undo':
      runUndo(Number.parseInt(resolved.args, 10) || 1);
      break;
    case 'btw': {
      if (!state.currentSessionId) {
        state.statusMsg = 'select a session first';
        return;
      }
      state.btwOpen = true;
      state.btwBusy = true;
      state.btwPrompt = resolved.args ?? '';
      state.btwAnswer = '';
      state.statusMsg = 'starting btw…';
      api('POST', `/api/v1/sessions/${state.currentSessionId}:btw`, {})
        .then(async (data) => {
          const agentId = (data?.data?.agent_id as string) ?? 'main';
          state.btwAgentId = agentId;
          state.btwBusy = false;
          state.statusMsg = '';
          if (resolved.args && resolved.args.trim().length > 0) {
            await api('POST', `/api/v1/sessions/${state.currentSessionId}/prompts`, {
              content: [{ type: 'text', text: resolved.args }],
              agent_id: agentId,
            });
          }
        })
        .catch((e) => {
          state.statusMsg = `btw failed: ${(e as Error).message}`;
        });
      break;
    }
    case 'model': {
      if (resolved.args) {
        const ref = resolved.args;
        api('POST', `/api/v1/models/${encodeURIComponent(ref)}:set_default`, {})
          .then((data) => {
            state.statusMsg = `default model → ${data?.data?.default_model ?? ref}`;
          })
          .catch((e) => {
            state.statusMsg = `model set failed: ${(e as Error).message}`;
          });
      } else {
        api('GET', `/api/v1/config`)
          .then((data) => {
            const c = (data?.data ?? {}) as Record<string, unknown>;
            state.statusMsg = `default model: ${(c.default_model as string) ?? '(unset)'}`;
          })
          .catch((e) => {
            state.statusMsg = `config failed: ${(e as Error).message}`;
          });
      }
      break;
    }
    case 'permission': {
      const mode = resolved.args || 'manual';
      if (!['manual', 'auto', 'yolo'].includes(mode)) {
        state.statusMsg = `permission: manual|auto|yolo (got ${mode})`;
        return;
      }
      api('POST', `/api/v1/config`, { default_permission_mode: mode })
        .then(() => {
          state.permissionMode = mode as State['permissionMode'];
          state.statusMsg = `permission mode → ${mode}`;
        })
        .catch((e) => {
          state.statusMsg = `permission failed: ${(e as Error).message}`;
        });
      break;
    }
    case 'yolo': {
      const on = resolved.args !== 'off';
      api('POST', `/api/v1/config`, { yolo: on })
        .then(() => {
          state.permissionMode = on ? 'yolo' : 'manual';
          state.statusMsg = `yolo ${on ? 'on' : 'off'}`;
        })
        .catch((e) => {
          state.statusMsg = `yolo failed: ${(e as Error).message}`;
        });
      break;
    }
    case 'auto': {
      const on = resolved.args !== 'off';
      api('POST', `/api/v1/config`, { default_permission_mode: on ? 'auto' : 'manual' })
        .then(() => {
          state.permissionMode = on ? 'auto' : 'manual';
          state.statusMsg = `auto ${on ? 'on' : 'off'}`;
        })
        .catch((e) => {
          state.statusMsg = `auto failed: ${(e as Error).message}`;
        });
      break;
    }
    case 'plan': {
      const on = resolved.args !== 'off' && resolved.args !== 'clear';
      api('POST', `/api/v1/config`, { default_plan_mode: on })
        .then(() => {
          state.statusMsg = `plan mode ${on ? 'on' : 'off'}`;
        })
        .catch((e) => {
          state.statusMsg = `plan failed: ${(e as Error).message}`;
        });
      break;
    }
    case 'effort': {
      const effort = resolved.args || 'off';
      api('POST', `/api/v1/config`, { thinking: { effort } })
        .then(() => {
          state.statusMsg = `thinking effort → ${effort}`;
        })
        .catch((e) => {
          state.statusMsg = `effort failed: ${(e as Error).message}`;
        });
      break;
    }
    case 'usage': {
      if (!state.currentSessionId) {
        state.statusMsg = 'select a session first';
        return;
      }
      api('GET', `/api/v1/sessions/${state.currentSessionId}/status`)
        .then((data) => {
          const st = (data?.data ?? {}) as Record<string, unknown>;
          state.statusMsg = `context ${st.context_tokens ?? '?'}/${st.max_context_tokens ?? '?'} (${st.context_usage ?? '?'}%) · thinking ${st.thinking_level ?? '?'} · plan ${st.plan_mode ?? '?'}`;
        })
        .catch((e) => {
          state.statusMsg = `usage failed: ${(e as Error).message}`;
        });
      break;
    }
    case 'tasks':
      dispatch(Msg.TasksOpen());
      break;
    case 'fork': {
      if (!state.currentSessionId) {
        state.statusMsg = 'select a session first';
        return;
      }
      api('POST', `/api/v1/sessions/${state.currentSessionId}:fork`, {})
        .then((data) => {
          const id = (data?.data?.id as string) ?? '';
          state.statusMsg = id ? `forked ${id}` : 'forked';
        })
        .catch((e) => {
          state.statusMsg = `fork failed: ${(e as Error).message}`;
        });
      break;
    }
    case 'title': {
      if (!state.currentSessionId) {
        state.statusMsg = 'select a session first';
        return;
      }
      if (!resolved.args) {
        const s = state.sessions.find((x) => x.id === state.currentSessionId);
        state.statusMsg = `title: ${s?.title ?? '(untitled)'}`;
        return;
      }
      api('POST', `/api/v1/sessions/${state.currentSessionId}/profile`, { title: resolved.args })
        .then(() => {
          state.statusMsg = `renamed to ${resolved.args}`;
        })
        .catch((e) => {
          state.statusMsg = `rename failed: ${(e as Error).message}`;
        });
      break;
    }
    case 'export-md': {
      if (!state.currentSessionId) {
        state.statusMsg = 'select a session first';
        return;
      }
      api('POST', `/api/v1/sessions/${state.currentSessionId}/export`, {})
        .then((data) => {
          const text = typeof data?.data === 'string' ? data.data : JSON.stringify(data?.data ?? {});
          void navigator.clipboard.writeText(text).then(() => {
            state.statusMsg = 'export copied to clipboard';
          });
        })
        .catch((e) => {
          state.statusMsg = `export failed: ${(e as Error).message}`;
        });
      break;
    }
    case 'reload': {
      if (state.currentSessionId) {
        void connectSession(state.currentSessionId);
        state.statusMsg = 'reloaded';
      } else {
        state.statusMsg = 'no session to reload';
      }
      break;
    }
    default:
      state.statusMsg = `/${cmd.name} is not wired in this client yet.`;
  }
}

export function applyTheme(theme: string): void {
  state.theme = theme;
  const root = document.documentElement;
  if (theme === 'light') {
    root.style.setProperty('--bg', '#ffffff');
    root.style.setProperty('--surface', '#f3f3f3');
    root.style.setProperty('--text', '#1e1e1e');
  } else if (theme === 'dark') {
    root.style.setProperty('--bg', '#181818');
    root.style.setProperty('--surface', '#212121');
    root.style.setProperty('--text', '#ffffff');
  }
  state.statusMsg = `theme ${theme}`;
}

export async function sendPrompt(text: string): Promise<void> {
  if (!state.currentSessionId) return;
  try {
    const data = await api('POST', `/api/v1/sessions/${state.currentSessionId}/prompts`, {
      content: [{ type: 'text', text }],
    });
    state.entries.push({ kind: 'user', text, streaming: false });
    state.entryCount = state.entries.length;
    const promptId = (data?.data?.prompt_id as string) ?? '';
    const steering = state.busy && state.busyInputMode === 'steer' && state.phase !== 'compacting' && promptId;
    if (steering) {
      await api('POST', `/api/v1/sessions/${state.currentSessionId}/prompts::steer`, {
        prompt_ids: [promptId],
      });
    }
    state.statusMsg = '';
    void refreshPromptQueue();
  } catch (e) {
    state.statusMsg = `send failed: ${(e as Error).message}`;
  }
}

async function refreshPromptQueue(): Promise<void> {
  if (!state.currentSessionId) return;
  try {
    const data = await api('GET', `/api/v1/sessions/${state.currentSessionId}/prompts`);
    const queued = (data?.data?.queued ?? []) as Record<string, unknown>[];
    state.queued = queued.map((p) => ({
      text: promptText(p),
      mode: 'prompt',
      promptId: (p.prompt_id as string) ?? '',
    }));
  } catch {
    /* non-fatal */
  }
}

function promptText(p: Record<string, unknown>): string {
  const parts = p?.content;
  if (typeof parts === 'string') return parts;
  if (Array.isArray(parts)) {
    return parts.map((x) => (x?.type === 'text' ? x.text : '')).filter(Boolean).join(' ');
  }
  return '';
}

function runShellCommand(text: string): void {
  if (!state.currentSessionId) {
    state.statusMsg = 'select a session first';
    return;
  }
  api('POST', `/api/v1/sessions/${state.currentSessionId}/shell`, { command: text.slice(1).trim() })
    .then(() => {
      state.statusMsg = '';
    })
    .catch((e) => {
      state.statusMsg = `shell failed: ${(e as Error).message}`;
    });
}

function drainQueuedBash(): void {
  const idx = state.queued.findIndex((q) => q.mode === 'bash');
  if (idx < 0) return;
  const item = state.queued[idx];
  state.queued.splice(idx, 1);
  runShellCommand(item.text);
}

export function sendBtw(text: string): void {
  if (!state.currentSessionId) return;
  const agentId = state.btwAgentId || 'main';
  state.btwPrompt = text;
  state.btwAnswer = '';
  state.btwBusy = true;
  api('POST', `/api/v1/sessions/${state.currentSessionId}/prompts`, {
    content: [{ type: 'text', text }],
    agent_id: agentId,
  })
    .then(() => {})
    .catch((e) => {
      state.btwBusy = false;
      state.btwAnswer = `error: ${(e as Error).message}`;
    });
}

export function runUndo(count: number): void {
  if (!state.currentSessionId) {
    state.statusMsg = 'select a session first';
    return;
  }
  api('POST', `/api/v1/sessions/${state.currentSessionId}:undo`, { count })
    .then((data) => {
      const msgs = (data?.data?.messages?.items ?? []) as Record<string, unknown>[];
      state.statusMsg = `undone (${msgs.length} messages)`;
      void connectSession(state.currentSessionId as string);
    })
    .catch((e) => {
      state.statusMsg = `undo failed: ${(e as Error).message}`;
    });
}

// ------------------------------------------------------------------ steer/cancel

function doSteer(): void {
  if (state.phase !== 'streaming') return;
  const draft = state.draft.trim();
  const draftIsBash = isBashDraft(draft);
  const submitDraftThenSteer = async (): Promise<void> => {
    const ids: string[] = [];
    if (!draftIsBash && draft.length > 0) {
      try {
        const data = await api('POST', `/api/v1/sessions/${state.currentSessionId}/prompts`, {
          content: [{ type: 'text', text: draft }],
        });
        const pid = (data?.data?.prompt_id as string) ?? '';
        if (pid) ids.push(pid);
      } catch (e) {
        state.statusMsg = `steer failed: ${(e as Error).message}`;
        return;
      }
    }
    for (const q of state.queued) {
      if (q.mode !== 'bash' && q.promptId) ids.push(q.promptId);
    }
    if (ids.length === 0) return;
    await api('POST', `/api/v1/sessions/${state.currentSessionId}/prompts::steer`, { prompt_ids: ids });
    if (!draftIsBash && draft.length > 0) state.draft = '';
    state.statusMsg = '';
    void refreshPromptQueue();
  };
  submitDraftThenSteer().catch((e) => {
    state.statusMsg = `steer failed: ${(e as Error).message}`;
  });
}

function doCancel(): void {
  if (state.busy) {
    const pid = state.currentPromptId;
    if (!pid) {
      api('POST', `/api/v1/sessions/${state.currentSessionId}:abort`, {}).catch(() => {});
      return;
    }
    api('POST', `/api/v1/sessions/${state.currentSessionId}/prompts/${pid}:abort`, {}).catch(() => {});
  }
}

// ------------------------------------------------------------------ approvals / questions

export function submitApproval(): void {
  const a = state.currentApproval;
  if (!a) return;
  const idx = state.approvalSelectedIndex;
  let decision = 'approved';
  let scope: string | undefined;
  if (idx === 1) scope = 'session';
  else if (idx === 2) decision = 'rejected';
  else if (idx === 3) {
    api('POST', `/api/v1/sessions/${state.currentSessionId}/approvals/${a.id}`, {
      decision: 'rejected',
      feedback: state.approvalFeedbackText || undefined,
    })
      .then(() => {
        state.currentApproval = null;
      })
      .catch((e) => {
        state.statusMsg = `approval failed: ${(e as Error).message}`;
      });
    return;
  }
  api('POST', `/api/v1/sessions/${state.currentSessionId}/approvals/${a.id}`, {
    decision,
    scope,
    selected_label: APPROVAL_CHOICES[idx]?.label,
  })
    .then(() => {
      state.currentApproval = null;
    })
    .catch((e) => {
      state.statusMsg = `approval failed: ${(e as Error).message}`;
    });
}

export function rejectApproval(): void {
  const a = state.currentApproval;
  if (!a) return;
  api('POST', `/api/v1/sessions/${state.currentSessionId}/approvals/${a.id}`, {
    decision: 'rejected',
  })
    .then(() => {
      state.currentApproval = null;
    })
    .catch(() => {
      state.currentApproval = null;
    });
}

export function dismissQuestion(): void {
  const q = state.currentQuestion;
  if (!q) return;
  api('POST', `/api/v1/sessions/${state.currentSessionId}/questions/${q.id}:dismiss`, {})
    .catch(() => {})
    .finally(() => {
      state.currentQuestion = null;
    });
}

export function submitQuestion(): void {
  const q = state.currentQuestion;
  if (!q) return;
  const answers: Record<string, unknown> = {};
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
  api('POST', `/api/v1/sessions/${state.currentSessionId}/questions/${q.id}`, { answers })
    .then(() => {
      state.currentQuestion = null;
    })
    .catch((e) => {
      state.statusMsg = `question failed: ${(e as Error).message}`;
    });
}

// ------------------------------------------------------------------ helpers

export function msgsToEntries(msgs: Record<string, unknown>[]): Entry[] {
  return msgs.map((m) => ({
    kind: m.role === 'user' ? 'user' : m.role === 'assistant' ? 'assistant' : 'status',
    text: contentToText(m.content),
  }));
}

function contentToText(content: unknown): string {
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

export async function createSession(): Promise<string | null> {
  try {
    const cur = state.sessions.find((s) => s.id === state.currentSessionId);
    const cwd = cur?.metadata?.cwd ?? state.currentCwd;
    const data = await api('POST', '/api/v1/sessions', { metadata: { cwd } });
    return (data?.data?.id as string) ?? (data?.id as string) ?? null;
  } catch (e) {
    state.statusMsg = `create failed: ${(e as Error).message}`;
    return null;
  }
}

const LAST_SESSION_KEY = 'dimi.lastSessionId';
export function readLastSessionId(): string {
  try {
    return localStorage.getItem(LAST_SESSION_KEY) ?? '';
  } catch {
    return '';
  }
}
export function rememberSession(id: string): void {
  try {
    localStorage.setItem(LAST_SESSION_KEY, id);
  } catch {
    /* non-fatal */
  }
}

// @mention completion
export async function maybeUpdateAtMention(text: string): Promise<void> {
  const m = text.match(/(?:^|\s)@([^\s]*)$/);
  if (!m) {
    state.atMentionOpen = false;
    return;
  }
  const prefix = m[1];
  try {
    const slashIdx = prefix.lastIndexOf('/');
    const dir = slashIdx >= 0 ? prefix.slice(0, slashIdx + 1) : (state.currentCwd || '.');
    const namePrefix = slashIdx >= 0 ? prefix.slice(slashIdx + 1) : prefix;
    const res = await dimi().listFs(dir || '.');
    if (!res?.ok) {
      state.atMentionOpen = false;
      return;
    }
    const entries = (res.entries ?? []) as { name: string; isDirectory: boolean; path: string }[];
    const filtered = entries
      .filter((e) => e.name.toLowerCase().startsWith(namePrefix.toLowerCase()))
      .map((e) => ({
        value: `@${dir}${e.name}${e.isDirectory ? '/' : ''}`,
        label: `${e.name}${e.isDirectory ? '/' : ''}`,
        description: e.isDirectory ? undefined : e.path,
      }));
    if (filtered.length === 0) {
      state.atMentionOpen = false;
      return;
    }
    state.atMentionOpen = true;
    state.atMentionPrefix = text.length - prefix.length - 1;
    state.completionItems = filtered;
    state.completionSelected = 0;
  } catch {
    state.atMentionOpen = false;
  }
}

// ------------------------------------------------------------------ boot

export async function boot(): Promise<void> {
  dispatch({ type: 'boot' });
  try {
    const meta = await api('GET', '/api/v1/meta');
    const serverVersion = (meta?.data?.server_version as string) ?? (meta?.data?.version as string) ?? '';
    const serverId = (meta?.data?.server_id as string) ?? (meta?.data?.id as string) ?? '';
    dispatch(Msg.SseConnected(serverVersion, serverId));
  } catch (e) {
    dispatch(Msg.SseError(String(e)));
  }
  void loadSessions();
}
