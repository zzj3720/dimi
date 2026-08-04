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
  currentPromptId: '',
  sessions: [],
  sessionsLoading: false,
  sessionsError: '',
  sessionsHasMore: false,
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
  planMode: false,
  permissionMode: 'manual', // manual | auto | yolo (footer mode badges)
  modelName: '',            // footer model label
  currentCwd: '',           // footer cwd
  footerTips: '/init: generate AGENTS.md | @: mention files', // footer line-1 tips (textMuted)
  footerContext: '',        // footer line-2 right context readout
  queued: [], // { text, mode }
  busyInputMode: DefaultBusyInputMode,
  draft: '',
  inputHistory: loadHistory(),
  historyIndex: -1,
  exitConfirmTicks: -1,
  tipTicks: 0,

  // interaction
  currentApproval: null,
  approvalSelectedIndex: 0,
  approvalFeedbackMode: false,
  approvalFeedbackText: '',
  approvalPreview: false,
  currentQuestion: null,
  questionSelectedIndex: 0,
  questionTabIndex: 0,
  questionOtherText: '',

  // completion (slash menu)
  completionOpen: false,
  completionItems: [], // { value, label, description }
  completionSelected: 0,
  completionPrefix: '',
  atMentionOpen: false,
  atMentionPrefix: 0,

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
  helpDialogOpen: false,
  theme: 'auto',

  // todo panel
  todoExpanded: false,

  // renderer-internal
  sseUnsubscribe: null,
  pendingUndoEscTicks: -1, // TUI double-Esc undo window (DOUBLE_ESC_WINDOW_MS)
  btwAgentId: '',
  undoRequested: false,
  approvalRejectRequested: false,
  questionDismissRequested: false,
  cancelStreamRequested: false,
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
  QuestionDismiss: () => ({ type: 'question_dismiss' }),
  QuestionTab: (delta) => ({ type: 'question_tab', delta }),
  QuestionToggle: (index) => ({ type: 'question_toggle', index }),
  QuestionConfirm: () => ({ type: 'question_confirm' }),
  QuestionOther: (text) => ({ type: 'question_other', text }),

  // ui
  Escape: () => ({ type: 'escape' }),
  ExpandToggle: () => ({ type: 'expand_toggle' }),
  ToolsExpandToggle: (toolCallId) => ({ type: 'tools_expand_toggle', toolCallId }),
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

    case 'sse_event':
      handleSseEvent(state, msg.evt);
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
      {
        const s = state.sessions.find((x) => x.id === msg.id);
        if (s) window.dimiCwd = s.metadata?.cwd ?? s.cwd ?? '';
      }
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
      if (!state.completionOpen && !state.atMentionOpen) return;
      {
        const item = state.completionItems[state.completionSelected];
        if (!item) return;
        // Replace from the completion prefix start (TUI applyCompletion:
        // prefix is the byte offset where the accepted value lands).
        const prefix = state.atMentionOpen ? state.atMentionPrefix : state.completionPrefix;
        state.draft = state.draft.slice(0, prefix) + item.value;
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
        // TUI layered cancel: first Ctrl+C clears a non-empty draft only;
        // only a second press (or already-empty draft) cancels the stream.
        if (state.draft) {
          state.draft = '';
          return;
        }
        state.cancelStreamRequested = true;
        state.statusMsg = 'cancelling…';
        return;
      }
      if (state.draft) {
        state.draft = '';
        return;
      }
      // idle + empty composer → exit confirm (1.5s window via 1s ticks)
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
        const n = APPROVAL_CHOICES.length;
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
        // Reject server-side (Esc/Ctrl+C/Ctrl+D all route here).
        state.approvalRejectRequested = true;
      }
      return;

    case 'question_dismiss':
      if (state.currentQuestion) {
        // Dismiss server-side (Esc/Ctrl+C/Ctrl+D).
        state.questionDismissRequested = true;
      }
      return;

    case 'approval_feedback':
      state.approvalFeedbackMode = true;
      state.approvalFeedbackText = msg.text;
      return;

    case 'question_move':
      if (state.currentQuestion) {
        // TUI: ↑/↓ moves the cursor on every question kind
        // (question-dialog.ts:161-167).
        const q = state.currentQuestion;
        const n = (q.options ?? []).length;
        if (n > 0) {
          state.questionSelectedIndex = (state.questionSelectedIndex + msg.delta + n) % n;
        }
      }
      return;

    case 'question_tab': {
      if (!state.currentQuestion) return;
      const total = (state.currentQuestion.allQuestions?.length ?? 1) + 1; // +1 submit tab
      const cur = state.currentQuestion.questionTabIndex ?? 0;
      state.currentQuestion.questionTabIndex = ((cur + msg.delta) % total + total) % total;
      // Sync the active question into the top-level fields for the view.
      syncQuestionTab(state);
      return;
    }

    case 'question_select':
      if (!state.currentQuestion) return;
      {
        const q = state.currentQuestion;
        const idx = msg.index;
        if (idx >= 0 && idx < (q.options ?? []).length) {
          q.options.forEach((o, i) => { o.selected = i === idx; });
          state.questionSelectedIndex = idx;
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
      // Keep the per-question copy so tab switches don't lose it.
      {
        const q = state.currentQuestion;
        if (q) {
          const all = q.allQuestions ?? [q];
          const cur = Math.min(q.questionTabIndex ?? 0, all.length - 1);
          all[cur].otherText = msg.text;
        }
      }
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
        // TUI session-picker: first Esc clears the query, second closes.
        if (state.pickerQuery.length > 0) {
          state.pickerQuery = '';
          state.pickerSelectedIndex = 0;
        } else {
          state.pickerOpen = false;
        }
        return;
      }
      if (state.settingsDialogOpen) {
        state.settingsDialogOpen = false;
        return;
      }
      if (state.helpDialogOpen) {
        state.helpDialogOpen = false;
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
        // TUI: Esc rejects the approval and tells the server
        // (approval-panel.ts:261-269 → {response:'rejected'}).
        state.approvalRejectRequested = true;
        return;
      }
      if (state.currentQuestion) {
        // TUI: Esc dismisses the question server-side (question-dialog.ts:128-131).
        state.questionDismissRequested = true;
        return;
      }
      if (state.btwOpen) {
        state.btwOpen = false;
        state.btwDraft = '';
        return;
      }
      if (state.busy) {
        // TUI: Esc cancels the current stream/compaction (editor-keyboard.ts:195-204).
        state.cancelStreamRequested = true;
        state.statusMsg = 'cancelling…';
        return;
      }
      // TUI double-Esc undo (editor-keyboard.ts:205-211): idle, no dialogs —
      // first Esc arms the window, second Esc within it undoes.
      if (state.pendingUndoEscTicks >= 0 && state.tipTicks - state.pendingUndoEscTicks <= 2) {
        state.pendingUndoEscTicks = -1;
        state.statusMsg = 'undo (double-esc)';
        state.undoRequested = true;
        return;
      }
      state.pendingUndoEscTicks = state.tipTicks;
      return;
    }

    case 'expand_toggle':
      state.displayMode =
        state.displayMode === 'summary' ? 'tools' : state.displayMode === 'tools' ? 'full' : 'summary';
      return;

    case 'tools_expand_toggle': {
      // Toggle a tool entry's output expansion (view.js click).
      const e = state.entries.find((x) => x.kind === 'tool' && x.toolCallId === msg.toolCallId);
      if (e) e.expanded = !e.expanded;
      return;
    }

    case 'plan_mode_toggle':
      state.planMode = !state.planMode;
      state.statusMsg = state.planMode ? 'plan mode on' : 'plan mode off';
      return;

    case 'todo_toggle':
      state.todoExpanded = !state.todoExpanded;
      return;

    case 'undo':
      state.statusMsg = 'undo';
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

// TUI persists input history to disk; the client uses localStorage.
const HISTORY_KEY = 'dimi.inputHistory';
function loadHistory() {
  try {
    const raw = localStorage.getItem(HISTORY_KEY);
    const parsed = raw ? JSON.parse(raw) : null;
    return Array.isArray(parsed) ? parsed : [];
  } catch {
    return [];
  }
}
function saveHistory(state) {
  try {
    localStorage.setItem(HISTORY_KEY, JSON.stringify(state.inputHistory.slice(-200)));
  } catch { /* non-fatal */ }
}

// TUI DEFAULT_APPROVAL_CHOICES (reverse-rpc/approval/adapter.ts) — the
// approval panel synthesizes these; the wire approval carries no options.
export const APPROVAL_CHOICES = [
  { label: 'Approve once' },
  { label: 'Approve for this session' },
  { label: 'Reject' },
  { label: 'Reject with feedback…' },
];

// Extract plain text from a prompt content parts array.
export function promptContentToText(content) {
  if (typeof content === 'string') return content;
  if (!Array.isArray(content)) return '';
  return content
    .map((part) => (part?.type === 'text' ? part.text ?? '' : ''))
    .filter(Boolean)
    .join(' ');
}

// SSE event → model. Mirrors the TUI's routeEvent (streaming-ui.ts): the
// envelope carries `type` on the payload (envelope.type is the SSE event
// name, payload.type is the event kind).
export function handleSseEvent(state, evt) {
  const p = evt.payload ?? evt;
  const type = p.type ?? evt.type;
  if (!type) return;

  switch (type) {
    case 'event.session.work_changed': {
      if (p.busy !== undefined) state.busy = !!p.busy;
      if (p.main_turn_active !== undefined) state.phase = p.main_turn_active ? 'streaming' : state.phase;
      if (p.pending_interaction !== undefined && p.pending_interaction !== 'none') {
        state.statusMsg = `waiting for ${p.pending_interaction}…`;
      }
      return;
    }

    case 'turn.started':
      state.busy = true;
      state.phase = 'streaming';
      state.statusMsg = '';
      state.currentPromptId = p.promptId ?? state.currentPromptId;
      return;

    case 'turn.ended': {
      state.busy = false;
      state.phase = 'idle';
      // Seal the final streaming entry.
      const last = state.entries[state.entries.length - 1];
      if (last && (last.kind === 'assistant' || last.kind === 'thinking') && last.streaming) {
        last.streaming = false;
      }
      if (p.reason === 'cancelled') state.statusMsg = 'cancelled';
      else if (p.reason === 'failed') state.statusMsg = `turn failed: ${p.error ?? ''}`;
      else state.statusMsg = '';
      return;
    }

    case 'assistant.delta': {
      const text = p.text ?? p.delta ?? '';
      if (!text) return;
      // BTW side-agent replies render in the BTW panel, not the transcript.
      if (state.btwOpen && state.btwAgentId && p.agentId === state.btwAgentId) {
        state.btwAnswer += text;
        state.btwBusy = false;
        return;
      }
      const last = state.entries[state.entries.length - 1];
      if (last && last.kind === 'assistant' && last.streaming) {
        last.text += text;
      } else {
        state.entries.push({ kind: 'assistant', text, streaming: true });
      }
      state.entryCount = state.entries.length;
      return;
    }

    case 'thinking.delta': {
      const text = p.text ?? p.delta ?? '';
      if (!text) return;
      const last = state.entries[state.entries.length - 1];
      if (last && last.kind === 'thinking' && last.streaming) {
        last.text += text;
      } else {
        state.entries.push({ kind: 'thinking', text, streaming: true });
      }
      return;
    }

    case 'tool.call.started': {
      state.entries.push({
        kind: 'tool',
        toolName: p.toolName ?? p.name ?? 'tool',
        toolCallId: p.toolCallId ?? p.id ?? '',
        args: typeof p.toolInput === 'string' ? p.toolInput
          : typeof p.tool_input === 'string' ? p.tool_input
          : typeof p.arguments === 'string' ? p.arguments
          : (p.toolInput ?? p.tool_input ?? p.args) ? JSON.stringify(p.toolInput ?? p.tool_input ?? p.args)
          : '',
        text: '',
        streaming: false,
      });
      state.entryCount = state.entries.length;
      return;
    }

    case 'tool.result': {
      for (let i = state.entries.length - 1; i >= 0; i--) {
        const e = state.entries[i];
        if (e.kind === 'tool' && e.toolCallId === (p.toolCallId ?? p.id ?? '')) {
          e.text = typeof p.output === 'string' ? p.output : JSON.stringify(p.output ?? '');
          e.streaming = false;
          return;
        }
      }
      return;
    }

    case 'prompt.submitted':
      state.currentPromptId = p.promptId ?? state.currentPromptId;
      state.statusMsg = '';
      // The user message is rendered locally on submit (main.js sendPrompt);
      // the server does not emit prompt.submitted on SSE, so no duplicate.
      return;

    case 'prompt.completed':
      state.currentPromptId = '';
      return;

    case 'prompt.aborted':
      state.currentPromptId = '';
      state.busy = false;
      state.phase = 'idle';
      return;

    case 'compaction.started':
      state.busy = true;
      state.phase = 'compacting';
      state.entries.push({ kind: 'compaction', text: 'compacting context…', streaming: false });
      return;

    case 'compaction.completed':
      state.busy = false;
      state.phase = 'idle';
      return;

    case 'compaction.cancelled':
      state.phase = 'idle';
      return;

    case 'event.approval.requested': {
      // The SSE payload carries the full wire approval. The client
      // synthesizes the choice list (TUI DEFAULT_APPROVAL_CHOICES).
      state.currentApproval = {
        id: p.approval_id ?? p.id ?? '',
        toolName: p.tool_name ?? '',
        action: p.action ?? '',
        command: typeof p.tool_input_display === 'string' ? p.tool_input_display : JSON.stringify(p.tool_input_display ?? ''),
        toolCallId: p.tool_call_id ?? '',
      };
      state.approvalSelectedIndex = 0;
      state.approvalFeedbackMode = false;
      state.approvalFeedbackText = '';
      return;
    }

    case 'event.approval.resolved':
      if (state.currentApproval && p.approval_id === state.currentApproval.id) {
        state.currentApproval = null;
      }
      return;

    case 'event.question.requested': {
      const questions = p.questions ?? [];
      if (questions.length === 0) return;
      // Support multiple questions with tabs (TUI question-dialog.ts:604-627).
      state.currentQuestion = {
        id: p.question_id ?? p.id ?? '',
        itemId: questions[0].id ?? '',
        question: questions[0].question ?? '',
        kind: questions[0].multi_select ? 'multi' : 'single',
        options: (questions[0].options ?? []).map((o) => ({ ...o, selected: false })),
        allowOther: !!questions[0].allow_other,
        otherLabel: questions[0].other_label ?? 'Other',
        allQuestions: questions.map((qq) => ({
          itemId: qq.id ?? '',
          question: qq.question ?? '',
          kind: qq.multi_select ? 'multi' : 'single',
          options: (qq.options ?? []).map((o) => ({ ...o, selected: false })),
          allowOther: !!qq.allow_other,
          otherLabel: qq.other_label ?? 'Other',
        })),
        questionTabIndex: 0,
      };
      state.questionSelectedIndex = 0;
      state.questionOtherText = '';
      return;
    }

    case 'event.question.answered':
    case 'event.question.dismissed':
      if (state.currentQuestion && p.question_id === state.currentQuestion.id) {
        state.currentQuestion = null;
      }
      return;

    case 'error': {
      const message = p.msg ?? p.message ?? 'server error';
      state.statusMsg = `error: ${message}`;
      return;
    }

    case 'session.meta.updated': {
      const s = state.sessions.find((x) => x.id === state.currentSessionId);
      if (s && p.title) s.title = p.title;
      return;
    }

    default:
      // Unknown event kinds are ignored by the POC (TUI ignores them too
      // unless a panel cares).
      return;
  }
}

export function filteredSessions(state) {
  let list = state.sessions;
  if (state.pickerScope === 'cwd') {
    // TUI: filter by the current workspace cwd. Derive it from the active
    // session's metadata (toWireSession: metadata.cwd).
    const active = state.sessions.find((s) => s.id === state.currentSessionId);
    const cwd = active?.metadata?.cwd ?? window.dimiCwd;
    list = list.filter((s) => (s.metadata?.cwd ?? s.cwd) === cwd);
  }
  // TUI: fuzzyFilter over title (searchable-list.ts) — the query is matched
  // token-wise with fuzzy scoring, not plain substring.
  return fuzzyFilter(list, state.pickerQuery, (s) => s.title || s.id || '');
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
    // Prefix = start of the argument being completed (after "/cmd ").
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
  // Prefix = 0: the whole "/prefix" is replaced by the accepted value.
  state.completionPrefix = 0;
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
  state.atMentionOpen = false;
  state.atMentionPrefix = 0;
}

// Sync the active question tab into the top-level currentQuestion fields
// the view reads (mirror of TUI gotoTab updating the visible question).
function syncQuestionTab(state) {
  const q = state.currentQuestion;
  if (!q || !q.allQuestions) return;
  const idx = q.questionTabIndex ?? 0;
  const total = q.allQuestions.length + 1;
  if (idx >= total) return; // submit tab — keep the last question visible
  const qq = q.allQuestions[idx];
  if (!qq) return;
  q.itemId = qq.itemId;
  q.question = qq.question;
  q.kind = qq.kind;
  q.options = qq.options;
  q.allowOther = qq.allowOther;
  q.otherLabel = qq.otherLabel;
  state.questionSelectedIndex = 0;
  state.questionOtherText = '';
}

export function isBashDraft(text) {
  return text.length > 0 && text[0] === '!';
}
