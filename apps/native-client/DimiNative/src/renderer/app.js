// Dimi native client — renderer core.
//
// Architecture mirrors the TUI 1:1: a `Model` (plain state), `Msg` (tagged
// union), and a pure `update(model, msg)` reducer. UI rendering is a thin
// projection of the model (imperative DOM here instead of pi-tui).
//
// Server transport goes through `window.dimi.request()` / `subscribeEvents()`
// bridged to the Electron main process (no CORS, token injected there).

// ------------------------------------------------------------------ state

export const BusyInputMode = {
  Steer: 'steer',
  Queue: 'queue',
};

export const DefaultBusyInputMode = BusyInputMode.Steer;

export const model = {
  // connection
  connection: 'connecting', // connecting | connected | failed
  statusMsg: 'connecting…',
  serverVersion: '',
  serverId: '',

  // session
  currentSessionId: '',
  sessions: [],
  sessionsLoading: false,
  sessionsError: '',
  pickerOpen: false,
  pickerQuery: '',
  pickerScope: 'all', // all | cwd
  pickerSelectedIndex: 0,

  // transcript
  entries: [], // { kind, text, toolName?, toolCallId?, folded?, truncated? }
  entryCount: 0,
  displayMode: 'summary', // summary | tools | full

  // chat
  busy: false,
  phase: 'idle', // idle | streaming | shell | compacting
  queued: [], // { text, mode }
  busyInputMode: DefaultBusyInputMode,
  draft: '',
  inputHistory: [],
  historyIndex: -1,
  exitConfirmTicks: -1,
  tipTicks: 0,

  // interaction
  currentApproval: null,
  approvalSelectedIndex: 0,
  approvalFeedbackMode: false,
  approvalFeedbackText: '',
  currentQuestion: null,
  questionSelectedIndex: 0,
  questionTabIndex: 0,
  questionOtherText: '',

  // completion (slash menu)
  completionOpen: false,
  completionItems: [], // { value, label, description }
  completionSelected: 0,
  completionPrefix: '',

  // btw panel
  btwOpen: false,
  btwDraft: '',
  btwPrompt: '',
  btwAnswer: '',
  btwBusy: false,

  // tasks browser
  tasksBrowserOpen: false,

  // settings
  settingsDialogOpen: false,

  // todo panel
  todoExpanded: false,

  // renderer-internal
  sseUnsubscribe: null,
};

// ------------------------------------------------------------------ msgs

export const Msg = {
  // transport
  SseConnected: (serverVersion, serverId) => ({ type: 'sse_connected', serverVersion, serverId }),
  SseEvent: (evt) => ({ type: 'sse_event', evt }),
  SseEnded: (outcome) => ({ type: 'sse_ended', outcome }),
  SseError: (message) => ({ type: 'sse_error', message }),

  // sessions
  SessionsLoaded: (sessions) => ({ type: 'sessions_loaded', sessions }),
  SessionSelected: (id) => ({ type: 'session_selected', id }),
  PickerOpen: () => ({ type: 'picker_open' }),
  PickerClose: () => ({ type: 'picker_close' }),
  PickerSearch: (query) => ({ type: 'picker_search', query }),
  PickerMove: (delta) => ({ type: 'picker_move', delta }),
  PickerSelect: () => ({ type: 'picker_select' }),
  PickerScope: (scope) => ({ type: 'picker_scope', scope }),

  // input
  DraftChange: (text) => ({ type: 'draft_change', text }),
  Submit: () => ({ type: 'submit' }),
  CompletionMove: (delta) => ({ type: 'completion_move', delta }),
  CompletionAccept: () => ({ type: 'completion_accept' }),
  CompletionClose: () => ({ type: 'completion_close' }),

  // chat
  Steer: () => ({ type: 'steer' }),
  Cancel: () => ({ type: 'cancel' }),
  SetBusyInputMode: (mode) => ({ type: 'set_busy_input_mode', mode }),
  HistoryPrev: () => ({ type: 'history_prev' }),
  HistoryNext: () => ({ type: 'history_next' }),

  // interactions
  ApprovalMove: (delta) => ({ type: 'approval_move', delta }),
  ApprovalSelect: (index) => ({ type: 'approval_select', index }),
  ApprovalConfirm: () => ({ type: 'approval_confirm' }),
  ApprovalReject: () => ({ type: 'approval_reject' }),
  ApprovalFeedback: (text) => ({ type: 'approval_feedback', text }),
  QuestionMove: (delta) => ({ type: 'question_move', delta }),
  QuestionToggle: (index) => ({ type: 'question_toggle', index }),
  QuestionConfirm: () => ({ type: 'question_confirm' }),
  QuestionOther: (text) => ({ type: 'question_other', text }),

  // ui
  Escape: () => ({ type: 'escape' }),
  ExpandToggle: () => ({ type: 'expand_toggle' }),
  Tick: () => ({ type: 'tick' }),
  TasksOpen: () => ({ type: 'tasks_open' }),
  TasksClose: () => ({ type: 'tasks_close' }),
  SettingsOpen: () => ({ type: 'settings_open' }),
  SettingsClose: () => ({ type: 'settings_close' }),
};

// ------------------------------------------------------------------ update

export function update(state, msg) {
  switch (msg.type) {
    case 'sse_connected':
      state.connection = 'connected';
      state.serverVersion = msg.serverVersion;
      state.statusMsg = `connected · v${msg.serverVersion}`;
      return;

    case 'sse_error':
      state.connection = 'failed';
      state.statusMsg = `SSE error: ${msg.message}`;
      return;

    case 'sse_ended':
      // Terminal SSE outcome: reconnect on unexpected death.
      if (msg.outcome !== 'cancelled' && msg.outcome !== 'rejected') {
        state.statusMsg = 'connection lost — reconnecting…';
        state.connection = 'connecting';
      }
      return;

    case 'sessions_loaded':
      state.sessions = msg.sessions;
      state.sessionsLoading = false;
      return;

    case 'session_selected':
      state.currentSessionId = msg.id;
      state.pickerOpen = false;
      state.entries = [];
      state.busy = false;
      state.phase = 'idle';
      state.statusMsg = `session ${msg.id}`;
      return;

    case 'picker_open':
      state.pickerOpen = true;
      state.pickerQuery = '';
      state.pickerSelectedIndex = 0;
      return;

    case 'picker_close':
      state.pickerOpen = false;
      return;

    case 'picker_search':
      state.pickerQuery = msg.query;
      state.pickerSelectedIndex = 0;
      return;

    case 'picker_move': {
      if (!state.pickerOpen) return;
      const n = filteredSessions(state).length;
      if (n === 0) return;
      state.pickerSelectedIndex =
        (state.pickerSelectedIndex + msg.delta + n) % n;
      return;
    }

    case 'picker_select':
      if (!state.pickerOpen) return;
      {
        const list = filteredSessions(state);
        if (list.length === 0) return;
        const s = list[Math.min(state.pickerSelectedIndex, list.length - 1)];
        state.currentSessionId = s.id;
        state.pickerOpen = false;
        state.entries = [];
        state.busy = false;
        state.phase = 'idle';
      }
      return;

    case 'picker_scope':
      state.pickerScope = msg.scope;
      state.pickerSelectedIndex = 0;
      return;

    // ------------------------------------------------ input / completion

    case 'draft_change':
      state.draft = msg.text;
      // typing cancels exit-confirm window (TUI §8)
      state.exitConfirmTicks = -1;
      updateCompletion(state);
      return;

    case 'completion_move':
      if (!state.completionOpen) return;
      if (state.completionItems.length === 0) return;
      state.completionSelected =
        (state.completionSelected + msg.delta + state.completionItems.length) %
        state.completionItems.length;
      return;

    case 'completion_accept':
      if (!state.completionOpen) return;
      {
        const item = state.completionItems[state.completionSelected];
        if (!item) return;
        // Replace the completion prefix with the accepted value.
        state.draft = state.draft.slice(0, state.completionPrefix.length) + item.value;
        closeCompletion(state);
      }
      return;

    case 'completion_close':
      closeCompletion(state);
      return;

    // ------------------------------------------------------------ chat

    case 'steer':
      // TUI: Ctrl+S in idle/shell/compacting does nothing.
      if (state.phase !== 'streaming') return;
      state.statusMsg = 'steer (ctrl-s)';
      return;

    case 'cancel':
      if (state.btwOpen) {
        state.btwOpen = false;
        state.btwDraft = '';
        return;
      }
      if (state.busy) {
        if (state.draft) state.draft = '';
        state.statusMsg = 'cancelling…';
        return;
      }
      if (state.draft) {
        state.draft = '';
        return;
      }
      // idle + empty composer → exit confirm (double-press window)
      if (state.exitConfirmTicks >= 0 && state.tipTicks - state.exitConfirmTicks <= 2) {
        window.close();
        return;
      }
      state.exitConfirmTicks = state.tipTicks;
      state.statusMsg = 'Press Ctrl+C again to exit';
      return;

    case 'set_busy_input_mode':
      state.busyInputMode = msg.mode;
      return;

    case 'history_prev': {
      if (state.historyIndex === -1) {
        // first press: save the current draft as the base
        state.historyIndex = state.inputHistory.length;
        state.historyBase = state.draft;
      }
      if (state.historyIndex > 0) {
        state.historyIndex -= 1;
        state.draft = state.inputHistory[state.historyIndex] ?? '';
        closeCompletion(state);
      }
      return;
    }

    case 'history_next': {
      if (state.historyIndex === -1) return;
      state.historyIndex += 1;
      if (state.historyIndex >= state.inputHistory.length) {
        state.historyIndex = state.inputHistory.length;
        state.draft = state.historyBase ?? '';
      } else {
        state.draft = state.inputHistory[state.historyIndex] ?? '';
      }
      closeCompletion(state);
      return;
    }

    // ----------------------------------------------------- interactions

    case 'approval_move':
      if (state.approvalFeedbackMode) {
        // TUI: ↑/↓ in feedback mode exits feedback and moves selection
        state.approvalFeedbackMode = false;
        state.approvalFeedbackText = '';
      }
      if (state.currentApproval) {
        const n = state.currentApproval.options.length;
        state.approvalSelectedIndex = (state.approvalSelectedIndex + msg.delta + n) % n;
      }
      return;

    case 'approval_select':
      state.approvalSelectedIndex = msg.index;
      return;

    case 'approval_confirm':
      if (!state.currentApproval) return;
      if (state.approvalFeedbackMode) {
        state.statusMsg = 'submitting feedback…';
        state.approvalFeedbackMode = false;
        return;
      }
      state.statusMsg = 'approving…';
      return;

    case 'approval_reject':
      if (state.currentApproval) {
        state.currentApproval = null;
        state.approvalFeedbackMode = false;
      }
      return;

    case 'approval_feedback':
      state.approvalFeedbackMode = true;
      state.approvalFeedbackText = msg.text;
      return;

    case 'question_move':
      if (state.currentQuestion) {
        const q = state.currentQuestion;
        if (q.kind === 'multi' || q.kind === 'multi_with_other') {
          const n = q.options.length;
          state.questionSelectedIndex = (state.questionSelectedIndex + msg.delta + n) % n;
        }
      }
      return;

    case 'question_toggle':
      if (!state.currentQuestion) return;
      {
        const q = state.currentQuestion;
        if (q.kind === 'multi' || q.kind === 'multi_with_other') {
          const idx = msg.index;
          q.options[idx].selected = !q.options[idx].selected;
        }
      }
      return;

    case 'question_confirm':
      if (state.currentQuestion) {
        state.statusMsg = 'answering…';
      }
      return;

    case 'question_other':
      state.questionOtherText = msg.text;
      return;

    // --------------------------------------------------------------- ui

    case 'escape': {
      // Layered Esc (TUI §8): picker → settings → approval feedback →
      // approval (reject) → question (dismiss) → btw → completion → cancel.
      if (state.completionOpen) {
        closeCompletion(state);
        return;
      }
      if (state.pickerOpen) {
        state.pickerOpen = false;
        return;
      }
      if (state.settingsDialogOpen) {
        state.settingsDialogOpen = false;
        return;
      }
      if (state.tasksBrowserOpen) {
        state.tasksBrowserOpen = false;
        return;
      }
      if (state.approvalFeedbackMode) {
        state.approvalFeedbackMode = false;
        state.approvalFeedbackText = '';
        return;
      }
      if (state.currentApproval) {
        state.currentApproval = null;
        state.approvalFeedbackMode = false;
        return;
      }
      if (state.currentQuestion) {
        state.currentQuestion = null;
        return;
      }
      if (state.btwOpen) {
        state.btwOpen = false;
        state.btwDraft = '';
        return;
      }
      if (state.busy) {
        state.statusMsg = 'cancelling…';
      }
      return;
    }

    case 'expand_toggle':
      state.displayMode =
        state.displayMode === 'summary' ? 'tools' : state.displayMode === 'tools' ? 'full' : 'summary';
      return;

    case 'tick':
      state.tipTicks += 1;
      return;

    case 'tasks_open':
      state.tasksBrowserOpen = true;
      return;

    case 'tasks_close':
      state.tasksBrowserOpen = false;
      return;

    case 'settings_open':
      state.settingsDialogOpen = true;
      return;

    case 'settings_close':
      state.settingsDialogOpen = false;
      return;

    default:
      return;
  }
}

// ------------------------------------------------------------------ helpers

export function filteredSessions(state) {
  const q = state.pickerQuery.trim().toLowerCase();
  let list = state.sessions;
  if (state.pickerScope === 'cwd') {
    list = list.filter((s) => s.cwd === window.dimiCwd);
  }
  if (!q) return list;
  return list.filter((s) =>
    (s.title || '').toLowerCase().includes(q) || (s.id || '').toLowerCase().includes(q),
  );
}

// ------------------------------------------------------------------ slash menu completion

// TUI command table (apps/dimi/src/tui/commands/registry.ts BUILTIN_SLASH_COMMANDS).
export const slashCommands = [
  { name: 'new', aliases: ['clear'], hint: '', desc: 'Start a new session', idleOnly: true },
  { name: 'sessions', aliases: ['resume'], hint: '', desc: 'Session picker', idleOnly: true },
  { name: 'fork', aliases: [], hint: '', desc: 'Fork the current session', idleOnly: true },
  { name: 'title', aliases: ['rename'], hint: '<title>', desc: 'Set / show session title' },
  { name: 'model', aliases: [], hint: '[name]', desc: 'Switch model' },
  { name: 'permission', aliases: [], hint: 'manual|yolo|auto', desc: 'Permission mode' },
  { name: 'yolo', aliases: ['yes'], hint: 'on|off', desc: 'YOLO permission' },
  { name: 'auto', aliases: [], hint: 'on|off', desc: 'Auto permission' },
  { name: 'plan', aliases: [], hint: 'on|off|clear', desc: 'Plan mode' },
  { name: 'swarm', aliases: [], hint: 'on|off|<task>', desc: 'Swarm mode', idleOnly: true },
  { name: 'compact', aliases: [], hint: '[instruction]', desc: 'Compact context', idleOnly: true },
  { name: 'undo', aliases: [], hint: '[count]', desc: 'Undo last user action', idleOnly: true },
  { name: 'theme', aliases: [], hint: 'auto|dark|light', desc: 'Set theme' },
  { name: 'settings', aliases: ['config'], hint: '', desc: 'Open settings menu' },
  { name: 'help', aliases: ['h', '?'], hint: '', desc: 'Help panel' },
  { name: 'exit', aliases: ['quit', 'q'], hint: '', desc: 'Exit' },
  { name: 'version', aliases: [], hint: '', desc: 'Show version' },
  { name: 'usage', aliases: [], hint: '', desc: 'Usage panel' },
  { name: 'status', aliases: [], hint: '', desc: 'Runtime status report' },
  { name: 'tasks', aliases: ['task'], hint: '', desc: 'Background task browser' },
  { name: 'mcp', aliases: [], hint: '', desc: 'MCP server status' },
  { name: 'plugins', aliases: [], hint: '', desc: 'Plugin management' },
  { name: 'add-dir', aliases: [], hint: '<path>|list', desc: 'Add additional working directory' },
  { name: 'experiments', aliases: ['experimental'], hint: '', desc: 'Experimental features', idleOnly: true },
  { name: 'reload', aliases: [], hint: '', desc: 'Reload session + config', idleOnly: true },
  { name: 'reload-tui', aliases: [], hint: '', desc: 'Reload TUI config' },
  { name: 'init', aliases: [], hint: '', desc: 'Generate AGENTS.md', idleOnly: true },
  { name: 'effort', aliases: ['thinking'], hint: 'off|low|high', desc: 'Thinking effort' },
  { name: 'provider', aliases: ['providers'], hint: '[name]|add|import|remove|refresh', desc: 'Provider management' },
  { name: 'login', aliases: [], hint: '[provider]', desc: 'Login to provider' },
  { name: 'logout', aliases: ['disconnect'], hint: '', desc: 'Logout provider' },
  { name: 'export-md', aliases: ['export'], hint: '', desc: 'Export session as Markdown' },
  { name: 'copy', aliases: [], hint: '', desc: 'Copy last assistant message' },
  { name: 'web', aliases: [], hint: '', desc: 'Open web UI' },
  { name: 'feedback', aliases: [], hint: '', desc: 'Submit feedback' },
  { name: 'editor', aliases: [], hint: '<command>', desc: 'Set external editor' },
  { name: 'btw', aliases: [], hint: '[prompt]', desc: 'Open BTW side chat' },
  { name: 'secondary_model', aliases: [], hint: '', desc: 'Configure secondary model for subagents' },
];

const argCompletions = {
  swarm: ['on', 'off'],
  permission: ['manual', 'yolo', 'auto'],
  plan: ['on', 'off', 'clear'],
  theme: ['auto', 'dark', 'light'],
  add_dir: ['list'],
};

export function findSlashCommand(name) {
  return slashCommands.find((c) => c.name === name || (c.aliases ?? []).includes(name)) ?? null;
}

// Mirror of pi-tui fuzzy.ts fuzzyMatch: all query chars in order; lower
// score = better.
function fuzzyMatch(query, text) {
  const q = query.toLowerCase();
  const t = text.toLowerCase();
  const matchQuery = (nq) => {
    if (nq.length === 0) return { matches: true, score: 0 };
    if (nq.length > t.length) return { matches: false, score: 0 };
    let qi = 0;
    let score = 0;
    let last = -1;
    let consec = 0;
    for (let i = 0; i < t.length && qi < nq.length; i++) {
      if (t[i] === nq[qi]) {
        const boundary = i === 0 || /[\s\-_./:]/.test(t[i - 1]);
        if (last === i - 1) {
          consec += 1;
          score -= consec * 5;
        } else {
          consec = 0;
          if (last >= 0) score += (i - last - 1) * 2;
        }
        if (boundary) score -= 10;
        score += i * 0.1;
        last = i;
        qi += 1;
      }
    }
    if (qi < nq.length) return { matches: false, score: 0 };
    if (nq === t) score -= 100;
    return { matches: true, score };
  };
  return matchQuery(q);
}

function fuzzyFilter(items, query, getText) {
  if (!query.trim()) return items;
  const tokens = query.trim().split(/[\s/]+/).filter((t) => t.length > 0);
  if (tokens.length === 0) return items;
  const results = [];
  for (const item of items) {
    const text = getText(item);
    let total = 0;
    let all = true;
    for (const token of tokens) {
      const m = fuzzyMatch(token, text);
      if (m.matches) total += m.score;
      else { all = false; break; }
    }
    if (all) results.push({ item, total });
  }
  results.sort((a, b) => a.total - b.total);
  return results.map((r) => r.item);
}

function slashCommandDescription(cmd) {
  const full = cmd.hint ? (cmd.desc ? `${cmd.hint} — ${cmd.desc}` : cmd.hint) : cmd.desc;
  return full || undefined;
}

export function updateCompletion(state) {
  const text = state.draft;
  if (text.length === 0 || text[0] !== '/' || isBashDraft(text)) {
    closeCompletion(state);
    return;
  }

  // Argument completion: `/cmd <prefix>` (TUI: after a space).
  const whitespaceMatch = text.match(/^\/(\S+)\s+(\S*)$/);
  if (whitespaceMatch) {
    const [, cmdName, argPrefix] = whitespaceMatch;
    const cmd = findSlashCommand(cmdName);
    if (!cmd) { closeCompletion(state); return; }
    const args = argCompletions[cmd.name] ?? [];
    const filtered = fuzzyFilter(args, argPrefix, (a) => a);
    if (filtered.length === 0) { closeCompletion(state); return; }
    state.completionOpen = true;
    state.completionPrefix = text.length - argPrefix.length;
    state.completionItems = filtered.map((a) => ({
      value: a,
      label: a,
      description: undefined,
    }));
    state.completionSelected = bestMatchIndex(filtered, argPrefix);
    return;
  }

  // Command-name completion: `/prefix`.
  const needle = text.slice(1);
  if (needle.includes(' ') || needle.includes('/')) {
    // Either args without leading space (rare) or a path — TUI treats a
    // path-like input as a path, not a command.
    closeCompletion(state);
    return;
  }
  const filtered = fuzzyFilter(slashCommands, needle, (c) => c.name);
  if (filtered.length === 0) { closeCompletion(state); return; }
  state.completionOpen = true;
  state.completionPrefix = 1; // keep the leading '/'
  state.completionItems = filtered.map((c) => ({
    value: `/${c.name}`,
    label: `/${c.name}`,
    description: slashCommandDescription(c),
  }));
  state.completionSelected = bestMatchIndex(filtered, needle);
}

function bestMatchIndex(items, prefix) {
  const p = prefix.toLowerCase();
  for (let i = 0; i < items.length; i++) {
    const name = typeof items[i].name === 'string' ? items[i].name : items[i];
    if (name.toLowerCase() === p) return i;
  }
  for (let i = 0; i < items.length; i++) {
    const name = typeof items[i].name === 'string' ? items[i].name : items[i];
    if (name.toLowerCase().startsWith(p)) return i;
  }
  return 0;
}

function closeCompletion(state) {
  state.completionOpen = false;
  state.completionItems = [];
  state.completionSelected = 0;
  state.completionPrefix = '';
}

export function isBashDraft(text) {
  return text.length > 0 && text[0] === '!';
}
