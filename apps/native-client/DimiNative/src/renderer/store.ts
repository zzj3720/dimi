// Dimi native client — Vue state store (migrated from app.js).
// A `State` (reactive), `Msg` tagged union, and `update(state, msg)` reducer.
// Vue components project the reactive state; transport lives in api.ts.

import { reactive } from 'vue';

// ------------------------------------------------------------------ types

export type ConnectionState = 'connecting' | 'connected' | 'failed';
export type Phase = 'idle' | 'streaming' | 'shell' | 'compacting';
export type BusyInputMode = 'steer' | 'queue';
export type PermissionMode = 'manual' | 'auto' | 'yolo';

export interface ToolRef {
  id: string;
  name: string;
  args: string;
  /** tool_result output; empty string = still in progress */
  text: string;
}

export interface Entry {
  kind: 'user' | 'assistant' | 'thinking' | 'tool' | 'status' | 'compaction';
  text: string;
  toolName?: string;
  toolCallId?: string;
  args?: string;
  /** For thinking entries: tool calls rendered INSIDE the reasoning disclosure
   * (Codex behavior) — history-load path merges tool_use into the thinking
   * entry; live SSE appends tool calls to the in-flight thinking entry. */
  tools?: ToolRef[];
  /** Live-streamed thinking duration in ms (Codex: "思考了 2m 0s"). */
  durationMs?: number;
  /** Message sent time (ms epoch). History load takes it from the wire
   * `created_at`; live-stream entries stamp Date.now() at creation. Rendered
   * as the hover-revealed codex sent time (design doc §1.7). */
  ts?: number;
  streaming?: boolean;
  folded?: boolean;
  expanded?: boolean;
}

export interface SessionSummary {
  id: string;
  title?: string | null;
  cwd?: string;
  metadata?: { cwd?: string; [k: string]: unknown };
  updated_at?: number;
  last_prompt?: string | null;
}

export interface Approval {
  id: string;
  toolName: string;
  action: string;
  command: string;
  toolCallId: string;
}

export interface Question {
  id: string;
  itemId: string;
  question: string;
  kind: 'single' | 'multi';
  options: { label: string; selected: boolean; [k: string]: unknown }[];
  allowOther: boolean;
  otherLabel: string;
  allQuestions?: Question[];
  questionTabIndex?: number;
  otherText?: string;
}

export interface CompletionItem {
  value: string;
  label: string;
  description?: string;
}

export interface State {
  connection: ConnectionState;
  statusMsg: string;
  serverVersion: string;
  serverId: string;

  currentSessionId: string;
  currentPromptId: string;
  sessions: SessionSummary[];
  sessionsLoading: boolean;
  sessionsError: string;
  sessionsHasMore: boolean;
  sidebarVisible: boolean;
  /** Live sidebar width (px), dragged in Sidebar.vue; HeaderBar's left slot mirrors it. */
  sidebarWidth: number;
  pickerOpen: boolean;
  pickerQuery: string;
  pickerScope: 'all' | 'cwd';
  pickerSelectedIndex: number;

  entries: Entry[];
  entryCount: number;
  displayMode: 'summary' | 'tools' | 'full';
  /** Live-stream thinking start timestamp (ms) for the duration badge. */
  thinkingStartTs: number;

  busy: boolean;
  phase: Phase;
  planMode: boolean;
  permissionMode: PermissionMode;
  modelName: string;
  effort: string;
  pinnedIds: string[]; // local pin simulation (server has no pin API)
  archivedIds: string[]; // local archive simulation // thinking effort off|low|high (codex Work model picker strength)
  currentCwd: string;
  footerTips: string;
  queued: { text: string; mode: string; promptId?: string }[];
  busyInputMode: BusyInputMode;
  draft: string;
  inputHistory: string[];
  attachments: { fileId: string; name: string }[]; // codex attachment chips (server /files uploads)
  historyIndex: number;
  historyBase?: string;
  exitConfirmTicks: number;
  tipTicks: number;

  currentApproval: Approval | null;
  approvalSelectedIndex: number;
  approvalFeedbackMode: boolean;
  approvalFeedbackText: string;
  approvalPreview: boolean;
  currentQuestion: Question | null;
  questionSelectedIndex: number;
  questionTabIndex: number;
  questionOtherText: string;

  completionOpen: boolean;
  completionItems: CompletionItem[];
  completionSelected: number;
  completionPrefix: number;
  atMentionOpen: boolean;
  atMentionPrefix: number;
  atMentionPending: number;

  btwOpen: boolean;
  btwDraft: string;
  btwPrompt: string;
  btwAnswer: string;
  btwBusy: boolean;

  tasksBrowserOpen: boolean;
  settingsDialogOpen: boolean;
  helpDialogOpen: boolean;
  theme: string;
  todoExpanded: boolean;

  sseUnsubscribe: null | (() => void);
  pendingUndoEscTicks: number;
  btwAgentId: string;
  lastCommandError: string;
  undoRequested: boolean;
  approvalRejectRequested: boolean;
  questionDismissRequested: boolean;
  cancelStreamRequested: boolean;
}

// ------------------------------------------------------------------ state

const HISTORY_KEY = 'dimi.inputHistory';

// Sidebar width (px): shared between Sidebar.vue (drag resize, persisted) and
// HeaderBar.vue (left-slot width). Codex clamps 240–520 with a 275 default.
export const SIDEBAR_WIDTH_KEY = 'dimi.sidebarWidth';
export const SIDEBAR_MIN_WIDTH = 240;
export const SIDEBAR_MAX_WIDTH = 520;
export const SIDEBAR_DEFAULT_WIDTH = 275;

export function clampSidebarWidth(w: number): number {
  return Math.min(SIDEBAR_MAX_WIDTH, Math.max(SIDEBAR_MIN_WIDTH, w));
}

function loadSidebarWidth(): number {
  try {
    const raw = Number(localStorage.getItem(SIDEBAR_WIDTH_KEY));
    const w = Number.isFinite(raw) && raw > 0 ? raw : SIDEBAR_DEFAULT_WIDTH;
    return clampSidebarWidth(w);
  } catch {
    return SIDEBAR_DEFAULT_WIDTH;
  }
}

function loadHistory(): string[] {
  try {
    const raw = localStorage.getItem(HISTORY_KEY);
    const parsed = raw ? JSON.parse(raw) : null;
    return Array.isArray(parsed) ? parsed : [];
  } catch {
    return [];
  }
}

export function saveHistory(state: State): void {
  try {
    localStorage.setItem(HISTORY_KEY, JSON.stringify(state.inputHistory.slice(-200)));
  } catch {
    /* non-fatal */
  }
}

export function createInitialState(): State {
  return {
    connection: 'connecting',
    statusMsg: 'connecting…',
    serverVersion: '',
    serverId: '',

    currentSessionId: '',
    currentPromptId: '',
    sessions: [],
    sessionsLoading: false,
    sessionsError: '',
    sessionsHasMore: false,
    sidebarVisible: true,
    sidebarWidth: loadSidebarWidth(),
    pickerOpen: false,
    pickerQuery: '',
    pickerScope: 'all',
    pickerSelectedIndex: 0,

    entries: [],
    entryCount: 0,
    thinkingStartTs: 0,
    displayMode: 'summary',

    busy: false,
    phase: 'idle',
    planMode: false,
    permissionMode: 'manual',
    modelName: '',
    effort: 'low',
    pinnedIds: (() => { try { return JSON.parse(localStorage.getItem('dimi.pinnedSessions') ?? '[]') as string[]; } catch { return []; } })(),
    archivedIds: (() => { try { return JSON.parse(localStorage.getItem('dimi.archivedSessions') ?? '[]') as string[]; } catch { return []; } })(),
    currentCwd: '',
    footerTips: '/init: generate AGENTS.md | @: mention files',
    queued: [],
    busyInputMode: 'steer',
    draft: '',
    inputHistory: loadHistory(),
    attachments: [],
    historyIndex: -1,
    exitConfirmTicks: -1,
    tipTicks: 0,

    currentApproval: null,
    approvalSelectedIndex: 0,
    approvalFeedbackMode: false,
    approvalFeedbackText: '',
    approvalPreview: false,
    currentQuestion: null,
    questionSelectedIndex: 0,
    questionTabIndex: 0,
    questionOtherText: '',

    completionOpen: false,
    completionItems: [],
    completionSelected: 0,
    completionPrefix: 0,
    atMentionOpen: false,
    atMentionPrefix: 0,
    atMentionPending: 0,

    btwOpen: false,
    btwDraft: '',
    btwPrompt: '',
    btwAnswer: '',
    btwBusy: false,

    tasksBrowserOpen: false,
    settingsDialogOpen: false,
    helpDialogOpen: false,
    theme: 'auto',
    todoExpanded: false,

    sseUnsubscribe: null,
    pendingUndoEscTicks: -1,
    btwAgentId: '',
    lastCommandError: '',
    undoRequested: false,
    approvalRejectRequested: false,
    questionDismissRequested: false,
    cancelStreamRequested: false,
  };
}

export const state: State = reactive(createInitialState());

// ------------------------------------------------------------------ msgs

export interface Msg {
  type: string;
  [k: string]: unknown;
}

export const Msg = {
  SseConnected: (serverVersion: string, serverId: string): Msg => ({ type: 'sse_connected', serverVersion, serverId }),
  SseEvent: (evt: unknown): Msg => ({ type: 'sse_event', evt }),
  SseEnded: (outcome: string): Msg => ({ type: 'sse_ended', outcome }),
  SseError: (message: string): Msg => ({ type: 'sse_error', message }),
  SessionsLoaded: (sessions: SessionSummary[]): Msg => ({ type: 'sessions_loaded', sessions }),
  SessionSelected: (id: string): Msg => ({ type: 'session_selected', id }),
  PickerOpen: (): Msg => ({ type: 'picker_open' }),
  PickerClose: (): Msg => ({ type: 'picker_close' }),
  PickerSearch: (query: string): Msg => ({ type: 'picker_search', query }),
  PickerMove: (delta: number): Msg => ({ type: 'picker_move', delta }),
  PickerSelect: (): Msg => ({ type: 'picker_select' }),
  PickerScope: (scope: 'all' | 'cwd'): Msg => ({ type: 'picker_scope', scope }),
  DraftChange: (text: string): Msg => ({ type: 'draft_change', text }),
  Submit: (): Msg => ({ type: 'submit' }),
  CompletionMove: (delta: number): Msg => ({ type: 'completion_move', delta }),
  CompletionAccept: (): Msg => ({ type: 'completion_accept' }),
  CompletionClose: (): Msg => ({ type: 'completion_close' }),
  Steer: (): Msg => ({ type: 'steer' }),
  Cancel: (): Msg => ({ type: 'cancel' }),
  Stop: (): Msg => ({ type: 'stop' }),
  SetBusyInputMode: (mode: BusyInputMode): Msg => ({ type: 'set_busy_input_mode', mode }),
  HistoryPrev: (): Msg => ({ type: 'history_prev' }),
  HistoryNext: (): Msg => ({ type: 'history_next' }),
  ApprovalMove: (delta: number): Msg => ({ type: 'approval_move', delta }),
  ApprovalSelect: (index: number): Msg => ({ type: 'approval_select', index }),
  ApprovalConfirm: (): Msg => ({ type: 'approval_confirm' }),
  ApprovalReject: (): Msg => ({ type: 'approval_reject' }),
  ApprovalFeedback: (text: string): Msg => ({ type: 'approval_feedback', text }),
  QuestionMove: (delta: number): Msg => ({ type: 'question_move', delta }),
  QuestionDismiss: (): Msg => ({ type: 'question_dismiss' }),
  QuestionTab: (delta: number): Msg => ({ type: 'question_tab', delta }),
  QuestionToggle: (index: number): Msg => ({ type: 'question_toggle', index }),
  QuestionConfirm: (): Msg => ({ type: 'question_confirm' }),
  QuestionOther: (text: string): Msg => ({ type: 'question_other', text }),
  Escape: (): Msg => ({ type: 'escape' }),
  ExpandToggle: (): Msg => ({ type: 'expand_toggle' }),
  ToolsExpandToggle: (toolCallId: string): Msg => ({ type: 'tools_expand_toggle', toolCallId }),
  Tick: (): Msg => ({ type: 'tick' }),
  TasksOpen: (): Msg => ({ type: 'tasks_open' }),
  TasksClose: (): Msg => ({ type: 'tasks_close' }),
  SettingsOpen: (): Msg => ({ type: 'settings_open' }),
  SettingsClose: (): Msg => ({ type: 'settings_close' }),
  SidebarResize: (width: number): Msg => ({ type: 'sidebar_resize', width }),
  PinToggle: (id: string): Msg => ({ type: 'pin_toggle', id }),
  ArchiveToggle: (id: string): Msg => ({ type: 'archive_toggle', id }),
};

// ------------------------------------------------------------------ helpers

export const APPROVAL_CHOICES = [
  { label: 'Approve once' },
  { label: 'Approve for this session' },
  { label: 'Reject' },
  { label: 'Reject with feedback…' },
];

export function promptContentToText(content: unknown): string {
  if (typeof content === 'string') return content;
  if (!Array.isArray(content)) return '';
  return content
    .map((part) => (part?.type === 'text' ? part.text ?? '' : ''))
    .filter(Boolean)
    .join(' ');
}

export function isBashDraft(text: string): boolean {
  return text.length > 0 && text[0] === '!';
}

// ------------------------------------------------------------------ update

export function update(s: State, msg: Msg): void {
  switch (msg.type) {
    case 'sse_connected':
      s.connection = 'connected';
      s.serverVersion = String(msg.serverVersion ?? '');
      s.statusMsg = `connected · v${s.serverVersion}`;
      return;

    case 'sse_error':
      s.connection = 'failed';
      s.statusMsg = `SSE error: ${String(msg.message ?? '')}`;
      return;

    case 'sse_ended':
      if (msg.outcome !== 'cancelled' && msg.outcome !== 'rejected') {
        s.statusMsg = 'connection lost — reconnecting…';
        s.connection = 'connecting';
      }
      return;

    case 'sse_event':
      handleSseEvent(s, msg.evt as unknown);
      return;

    case 'sessions_loaded':
      s.sessions = (msg.sessions as SessionSummary[]) ?? [];
      s.sessionsLoading = false;
      return;

    case 'sidebar_toggle':
      s.sidebarVisible = !s.sidebarVisible;
      return;

    case 'pin_toggle': {
      const id = msg.id as string;
      s.pinnedIds = s.pinnedIds.includes(id) ? s.pinnedIds.filter((x) => x !== id) : [...s.pinnedIds, id];
      try { localStorage.setItem('dimi.pinnedSessions', JSON.stringify(s.pinnedIds)); } catch { /* non-fatal */ }
      return;
    }
    case 'archive_toggle': {
      const id = msg.id as string;
      s.archivedIds = s.archivedIds.includes(id) ? s.archivedIds.filter((x) => x !== id) : [...s.archivedIds, id];
      try { localStorage.setItem('dimi.archivedSessions', JSON.stringify(s.archivedIds)); } catch { /* non-fatal */ }
      return;
    }
    case 'sidebar_resize': {
      const w = Number(msg.width);
      if (!Number.isFinite(w)) return;
      s.sidebarWidth = clampSidebarWidth(w);
      return;
    }

    case 'session_selected':
      s.currentSessionId = String(msg.id ?? '');
      s.pickerOpen = false;
      s.entries = [];
      s.busy = false;
      s.phase = 'idle';
      s.statusMsg = '';
      // A freshly created session may not be in the sidebar list yet (e.g.
      // /new or the new-chat button) — surface it so the sidebar reflects it.
      if (!s.sessions.some((x) => x.id === s.currentSessionId)) {
        s.sessions.unshift({ id: s.currentSessionId, title: '' });
      }
      {
        const sess = s.sessions.find((x) => x.id === s.currentSessionId);
        if (sess) s.currentCwd = sess.metadata?.cwd ?? sess.cwd ?? '';
      }
      return;

    case 'picker_open':
      s.pickerOpen = true;
      s.pickerQuery = '';
      s.pickerSelectedIndex = 0;
      return;

    case 'picker_close':
      s.pickerOpen = false;
      return;

    case 'picker_search':
      s.pickerQuery = String(msg.query ?? '');
      s.pickerSelectedIndex = 0;
      return;

    case 'picker_move': {
      if (!s.pickerOpen) return;
      const n = filteredSessions(s).length;
      if (n === 0) return;
      s.pickerSelectedIndex = (s.pickerSelectedIndex + Number(msg.delta) + n) % n;
      return;
    }

    case 'picker_select':
      if (!s.pickerOpen) return;
      {
        const list = filteredSessions(s);
        if (list.length === 0) return;
        const sess = list[Math.min(s.pickerSelectedIndex, list.length - 1)];
        s.currentSessionId = sess.id;
        s.pickerOpen = false;
        s.entries = [];
        s.busy = false;
        s.phase = 'idle';
      }
      return;

    case 'picker_scope':
      s.pickerScope = msg.scope as 'all' | 'cwd';
      s.pickerSelectedIndex = 0;
      return;

    // ------------------------------------------------ input / completion

    case 'draft_change':
      s.draft = String(msg.text ?? '');
      s.exitConfirmTicks = -1;
      updateCompletion(s);
      return;

    case 'completion_move':
      if (!s.completionOpen) return;
      if (s.completionItems.length === 0) return;
      s.completionSelected =
        (s.completionSelected + Number(msg.delta) + s.completionItems.length) % s.completionItems.length;
      return;

    case 'completion_accept':
      if (!s.completionOpen && !s.atMentionOpen) return;
      {
        const item = s.completionItems[s.completionSelected];
        if (!item) return;
        const prefix = s.atMentionOpen ? s.atMentionPrefix : s.completionPrefix;
        s.draft = s.draft.slice(0, prefix) + item.value;
        closeCompletion(s);
      }
      return;

    case 'completion_close':
      closeCompletion(s);
      return;

    // ------------------------------------------------------------ chat

    case 'steer':
      if (s.phase !== 'streaming') return;
      s.statusMsg = 'steer (ctrl-s)';
      return;

    case 'cancel':
      if (s.btwOpen) {
        s.btwOpen = false;
        s.btwDraft = '';
        return;
      }
      if (s.busy) {
        if (s.draft) {
          s.draft = '';
          return;
        }
        s.cancelStreamRequested = true;
        s.statusMsg = 'cancelling…';
        return;
      }
      if (s.draft) {
        s.draft = '';
        return;
      }
      if (s.exitConfirmTicks >= 0 && s.tipTicks - s.exitConfirmTicks <= 2) {
        window.close();
        return;
      }
      s.exitConfirmTicks = s.tipTicks;
      s.statusMsg = 'Press Ctrl+C again to exit';
      return;

    case 'stop':
      // Composer stop button: unlike `cancel` (keyboard Ctrl+C, which clears
      // the draft first), stop always aborts the running turn immediately.
      if (s.busy) {
        s.cancelStreamRequested = true;
        s.statusMsg = 'cancelling…';
      }
      return;

    case 'set_busy_input_mode':
      s.busyInputMode = msg.mode as BusyInputMode;
      return;

    case 'history_prev': {
      if (s.historyIndex === -1) {
        s.historyIndex = s.inputHistory.length;
        s.historyBase = s.draft;
      }
      if (s.historyIndex > 0) {
        s.historyIndex -= 1;
        s.draft = s.inputHistory[s.historyIndex] ?? '';
        closeCompletion(s);
      }
      return;
    }

    case 'history_next': {
      if (s.historyIndex === -1) return;
      s.historyIndex += 1;
      if (s.historyIndex >= s.inputHistory.length) {
        s.historyIndex = s.inputHistory.length;
        s.draft = s.historyBase ?? '';
      } else {
        s.draft = s.inputHistory[s.historyIndex] ?? '';
      }
      closeCompletion(s);
      return;
    }

    // ----------------------------------------------------- interactions

    case 'approval_move':
      if (s.approvalFeedbackMode) {
        s.approvalFeedbackMode = false;
        s.approvalFeedbackText = '';
      }
      if (s.currentApproval) {
        const n = APPROVAL_CHOICES.length;
        s.approvalSelectedIndex = (s.approvalSelectedIndex + Number(msg.delta) + n) % n;
      }
      return;

    case 'approval_select':
      s.approvalSelectedIndex = Number(msg.index);
      return;

    case 'approval_confirm':
      if (!s.currentApproval) return;
      if (s.approvalFeedbackMode || s.approvalSelectedIndex === 3) {
        // Never submit a feedback-less rejection. Enter can reach this from
        // the Composer textarea or the feedback input while the feedback is
        // still empty (double-click / focus race / Esc then re-Enter, which
        // clears the mode flag but leaves the selection on row 3), and it
        // would otherwise fall back to a plain rejection; stay in feedback
        // mode instead. submitApproval() enforces the same invariant at the
        // effect layer, since afterDispatch runs unconditionally.
        if (!s.approvalFeedbackText.trim()) {
          s.statusMsg = 'enter feedback first';
          return;
        }
        s.statusMsg = 'submitting feedback…';
        s.approvalFeedbackMode = false;
        return;
      }
      s.statusMsg = 'approving…';
      return;

    case 'approval_reject':
      if (s.currentApproval) {
        s.approvalRejectRequested = true;
      }
      return;

    case 'question_dismiss':
      if (s.currentQuestion) {
        s.questionDismissRequested = true;
      }
      return;

    case 'approval_feedback':
      s.approvalFeedbackMode = true;
      s.approvalFeedbackText = String(msg.text ?? '');
      return;

    case 'question_move':
      if (s.currentQuestion) {
        const q = s.currentQuestion;
        const n = (q.options ?? []).length;
        if (n > 0) {
          s.questionSelectedIndex = (s.questionSelectedIndex + Number(msg.delta) + n) % n;
        }
      }
      return;

    case 'question_tab': {
      if (!s.currentQuestion) return;
      const total = (s.currentQuestion.allQuestions?.length ?? 1) + 1;
      const cur = s.currentQuestion.questionTabIndex ?? 0;
      s.currentQuestion.questionTabIndex = ((cur + Number(msg.delta)) % total + total) % total;
      syncQuestionTab(s);
      return;
    }

    case 'question_select':
      if (!s.currentQuestion) return;
      {
        const q = s.currentQuestion;
        const idx = Number(msg.index);
        if (idx >= 0 && idx < (q.options ?? []).length) {
          q.options.forEach((o, i) => {
            o.selected = i === idx;
          });
          s.questionSelectedIndex = idx;
        }
      }
      return;

    case 'question_toggle':
      if (!s.currentQuestion) return;
      {
        const q = s.currentQuestion;
        if (q.kind === 'multi' || q.kind === 'multi_with_other') {
          const idx = Number(msg.index);
          q.options[idx].selected = !q.options[idx].selected;
        }
      }
      return;

    case 'question_confirm':
      if (s.currentQuestion) {
        s.statusMsg = 'answering…';
      }
      return;

    case 'question_other':
      s.questionOtherText = String(msg.text ?? '');
      {
        const q = s.currentQuestion;
        if (q) {
          const all = q.allQuestions ?? [q];
          const cur = Math.min(q.questionTabIndex ?? 0, all.length - 1);
          all[cur].otherText = s.questionOtherText;
        }
      }
      return;

    // --------------------------------------------------------------- ui

    case 'escape': {
      if (s.completionOpen) {
        closeCompletion(s);
        return;
      }
      if (s.pickerOpen) {
        if (s.pickerQuery.length > 0) {
          s.pickerQuery = '';
          s.pickerSelectedIndex = 0;
        } else {
          s.pickerOpen = false;
        }
        return;
      }
      if (s.settingsDialogOpen) {
        s.settingsDialogOpen = false;
        return;
      }
      if (s.helpDialogOpen) {
        s.helpDialogOpen = false;
        return;
      }
      if (s.tasksBrowserOpen) {
        s.tasksBrowserOpen = false;
        return;
      }
      if (s.approvalFeedbackMode) {
        s.approvalFeedbackMode = false;
        s.approvalFeedbackText = '';
        return;
      }
      if (s.currentApproval) {
        s.approvalRejectRequested = true;
        return;
      }
      if (s.currentQuestion) {
        s.questionDismissRequested = true;
        return;
      }
      if (s.btwOpen) {
        s.btwOpen = false;
        s.btwDraft = '';
        return;
      }
      if (s.busy) {
        s.cancelStreamRequested = true;
        s.statusMsg = 'cancelling…';
        return;
      }
      if (s.pendingUndoEscTicks >= 0 && s.tipTicks - s.pendingUndoEscTicks <= 2) {
        s.pendingUndoEscTicks = -1;
        s.statusMsg = 'undo (double-esc)';
        s.undoRequested = true;
        return;
      }
      s.pendingUndoEscTicks = s.tipTicks;
      return;
    }

    case 'expand_toggle':
      s.displayMode =
        s.displayMode === 'summary' ? 'tools' : s.displayMode === 'tools' ? 'full' : 'summary';
      return;

    case 'tools_expand_toggle': {
      const e = s.entries.find((x) => x.kind === 'tool' && x.toolCallId === String(msg.toolCallId));
      if (e) e.expanded = !e.expanded;
      return;
    }

    case 'plan_mode_toggle':
      s.planMode = !s.planMode;
      s.statusMsg = s.planMode ? 'plan mode on' : 'plan mode off';
      return;

    case 'todo_toggle':
      s.todoExpanded = !s.todoExpanded;
      return;

    case 'undo':
      s.statusMsg = 'undo';
      return;

    case 'tick':
      s.tipTicks += 1;
      return;

    case 'tasks_open':
      s.tasksBrowserOpen = true;
      return;

    case 'tasks_close':
      s.tasksBrowserOpen = false;
      return;

    case 'settings_open':
      s.settingsDialogOpen = true;
      return;

    case 'settings_close':
      s.settingsDialogOpen = false;
      return;

    default:
      return;
  }
}

// ------------------------------------------------------------------ SSE events

export function handleSseEvent(s: State, evt: unknown): void {
  const e = evt as { payload?: Record<string, unknown>; type?: string };
  const p = (e.payload ?? e) as Record<string, unknown>;
  const type = (p.type as string) ?? e.type;
  if (!type) return;

  switch (type) {
    case 'event.session.work_changed': {
      if (p.busy !== undefined) s.busy = !!p.busy;
      if (p.main_turn_active !== undefined) s.phase = p.main_turn_active ? 'streaming' : s.phase;
      if (p.pending_interaction !== undefined && p.pending_interaction !== 'none') {
        s.statusMsg = `waiting for ${String(p.pending_interaction)}…`;
      }
      return;
    }

    case 'turn.started':
      s.busy = true;
      s.phase = 'streaming';
      s.statusMsg = '';
      s.currentPromptId = String(p.promptId ?? s.currentPromptId);
      return;

    case 'turn.ended': {
      s.busy = false;
      s.phase = 'idle';
      const last = s.entries[s.entries.length - 1];
      if (last && (last.kind === 'assistant' || last.kind === 'thinking') && last.streaming) {
        last.streaming = false;
      }
      if (p.reason === 'cancelled') s.statusMsg = 'cancelled';
      else if (p.reason === 'failed') s.statusMsg = `turn failed: ${String(p.error ?? '')}`;
      else s.statusMsg = '';
      return;
    }

    case 'assistant.delta': {
      const text = (p.text as string) ?? (p.delta as string) ?? '';
      if (!text) return;
      if (s.btwOpen && s.btwAgentId && p.agentId === s.btwAgentId) {
        s.btwAnswer += text;
        s.btwBusy = false;
        return;
      }
      const last = s.entries[s.entries.length - 1];
      if (last && last.kind === 'assistant' && last.streaming) {
        last.text += text;
      } else {
        // First assistant text chunk: thinking is over — stamp the duration.
        if (s.thinkingStartTs > 0) {
          const dur = Date.now() - s.thinkingStartTs;
          s.thinkingStartTs = 0;
          const t = s.entries[s.entries.length - 1];
          if (t && t.kind === 'thinking') t.durationMs = dur;
        }
        s.entries.push({ kind: 'assistant', text, streaming: true, ts: Date.now() });
      }
      s.entryCount = s.entries.length;
      return;
    }

    case 'thinking.delta': {
      const text = (p.text as string) ?? (p.delta as string) ?? '';
      if (!text) return;
      const last = s.entries[s.entries.length - 1];
      if (last && last.kind === 'thinking' && last.streaming) {
        last.text += text;
      } else {
        // First thinking chunk of this turn: start the duration clock.
        if (s.thinkingStartTs === 0) s.thinkingStartTs = Date.now();
        s.entries.push({ kind: 'thinking', text, streaming: true, ts: Date.now() });
      }
      return;
    }

    case 'tool.call.started': {
      const ti = p.toolInput ?? p.tool_input ?? p.args;
      const id = (p.toolCallId as string) ?? (p.id as string) ?? '';
      const name = (p.toolName as string) ?? (p.name as string) ?? 'tool';
      const args =
        typeof p.toolInput === 'string'
          ? p.toolInput
          : typeof p.tool_input === 'string'
            ? p.tool_input
            : typeof p.arguments === 'string'
              ? p.arguments
              : ti
                ? JSON.stringify(ti)
                : '';
      // Codex renders tool calls INSIDE the reasoning disclosure: attach to
      // the current turn's thinking entry when one exists (live path).
      for (let i = s.entries.length - 1; i >= 0; i--) {
        const e = s.entries[i];
        if (e.kind === 'assistant' || e.kind === 'user') break; // past this turn
        if (e.kind === 'thinking') {
          const arr = e.tools ?? (e.tools = []);
          const ex = arr.find((t) => t.id === id);
          if (ex) {
            if (args) ex.args = args;
          } else {
            arr.push({ id, name, args, text: '' });
          }
          return;
        }
      }
      // Fallback: no thinking entry in the current turn — standalone tool row.
      const existing = s.entries.find((e) => e.kind === 'tool' && e.toolCallId === id);
      if (existing) {
        if (args) existing.args = args;
        return;
      }
      s.entries.push({
        kind: 'tool',
        toolName: name,
        toolCallId: id,
        args,
        text: '',
        streaming: false,
        ts: Date.now(),
      });
      s.entryCount = s.entries.length;
      return;
    }

    case 'tool.result': {
      const id = (p.toolCallId as string) ?? (p.id as string) ?? '';
      const output = typeof p.output === 'string' ? p.output : JSON.stringify(p.output ?? '');
      for (let i = s.entries.length - 1; i >= 0; i--) {
        const e = s.entries[i];
        if (e.kind === 'thinking' && e.tools) {
          const t = e.tools.find((x) => x.id === id);
          if (t) {
            t.text = output;
            return;
          }
        }
        if (e.kind === 'tool' && e.toolCallId === id) {
          e.text = output;
          e.streaming = false;
          return;
        }
      }
      return;
    }

    case 'prompt.submitted':
      s.currentPromptId = (p.promptId as string) ?? s.currentPromptId;
      s.statusMsg = '';
      return;

    case 'prompt.completed':
      s.currentPromptId = '';
      return;

    case 'prompt.aborted':
      s.currentPromptId = '';
      s.busy = false;
      s.phase = 'idle';
      return;

    case 'compaction.started':
      s.busy = true;
      s.phase = 'compacting';
      s.entries.push({ kind: 'compaction', text: 'compacting context…', streaming: false });
      return;

    case 'compaction.completed':
      s.busy = false;
      s.phase = 'idle';
      return;

    case 'compaction.cancelled':
      s.phase = 'idle';
      return;

    case 'event.approval.requested': {
      s.currentApproval = {
        id: (p.approval_id as string) ?? (p.id as string) ?? '',
        toolName: (p.tool_name as string) ?? '',
        action: (p.action as string) ?? '',
        command:
          typeof p.tool_input_display === 'string'
            ? p.tool_input_display
            : JSON.stringify(p.tool_input_display ?? ''),
        toolCallId: (p.tool_call_id as string) ?? '',
      };
      s.approvalSelectedIndex = 0;
      s.approvalFeedbackMode = false;
      s.approvalFeedbackText = '';
      s.approvalPreview = false;
      return;
    }

    case 'event.approval.resolved':
      if (s.currentApproval && p.approval_id === s.currentApproval.id) {
        s.currentApproval = null;
      }
      return;

    case 'event.question.requested': {
      const questions = (p.questions as Record<string, unknown>[]) ?? [];
      if (questions.length === 0) return;
      const mapQ = (qq: Record<string, unknown>): Question => ({
        itemId: (qq.id as string) ?? '',
        question: (qq.question as string) ?? '',
        kind: qq.multi_select ? 'multi' : 'single',
        options: ((qq.options as Record<string, unknown>[]) ?? []).map((o) => ({ ...o, selected: false })),
        allowOther: !!qq.allow_other,
        otherLabel: (qq.other_label as string) ?? 'Other',
      });
      s.currentQuestion = {
        id: (p.question_id as string) ?? (p.id as string) ?? '',
        ...mapQ(questions[0]),
        allQuestions: questions.map(mapQ),
        questionTabIndex: 0,
      };
      s.questionSelectedIndex = 0;
      s.questionOtherText = '';
      return;
    }

    case 'event.question.answered':
    case 'event.question.dismissed':
      if (s.currentQuestion && p.question_id === s.currentQuestion.id) {
        s.currentQuestion = null;
      }
      return;

    case 'error': {
      const message = (p.msg as string) ?? (p.message as string) ?? 'server error';
      s.statusMsg = `error: ${message}`;
      return;
    }

    case 'session.meta.updated': {
      const sess = s.sessions.find((x) => x.id === s.currentSessionId);
      if (sess && p.title) sess.title = p.title as string;
      return;
    }

    default:
      return;
  }
}

// ------------------------------------------------------------------ session filtering

export function filteredSessions(s: State): SessionSummary[] {
  let list = s.sessions;
  if (s.pickerScope === 'cwd') {
    const active = s.sessions.find((x) => x.id === s.currentSessionId);
    const cwd = active?.metadata?.cwd ?? s.currentCwd;
    list = list.filter((x) => (x.metadata?.cwd ?? x.cwd) === cwd);
  }
  return fuzzyFilter(list, s.pickerQuery, (x) => x.title || x.id || '');
}

// ------------------------------------------------------------------ slash commands

export interface SlashCommand {
  name: string;
  aliases?: string[];
  hint?: string;
  desc: string;
  idleOnly?: boolean;
}

export const slashCommands: SlashCommand[] = [
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

const argCompletions: Record<string, string[]> = {
  swarm: ['on', 'off'],
  permission: ['manual', 'yolo', 'auto'],
  plan: ['on', 'off', 'clear'],
  theme: ['auto', 'dark', 'light'],
  add_dir: ['list'],
};

export function findSlashCommand(name: string): SlashCommand | null {
  return slashCommands.find((c) => c.name === name || (c.aliases ?? []).includes(name)) ?? null;
}

function fuzzyMatch(query: string, text: string): { matches: boolean; score: number } {
  const q = query.toLowerCase();
  const t = text.toLowerCase();
  const matchQuery = (nq: string): { matches: boolean; score: number } => {
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

function fuzzyFilter<T>(items: T[], query: string, getText: (item: T) => string): T[] {
  if (!query.trim()) return items;
  const tokens = query.trim().split(/[\s/]+/).filter((t) => t.length > 0);
  if (tokens.length === 0) return items;
  const results: { item: T; total: number }[] = [];
  for (const item of items) {
    const text = getText(item);
    let total = 0;
    let all = true;
    for (const token of tokens) {
      const m = fuzzyMatch(token, text);
      if (m.matches) total += m.score;
      else {
        all = false;
        break;
      }
    }
    if (all) results.push({ item, total });
  }
  results.sort((a, b) => a.total - b.total);
  return results.map((r) => r.item);
}

function slashCommandDescription(cmd: SlashCommand): string | undefined {
  const full = cmd.hint ? (cmd.desc ? `${cmd.hint} — ${cmd.desc}` : cmd.hint) : cmd.desc;
  return full || undefined;
}

export function updateCompletion(s: State): void {
  const text = s.draft;
  if (text.length === 0) {
    closeCompletion(s);
    return;
  }
  // @-mention drafts are driven by the async fsList result
  // (maybeUpdateAtMention in api.ts): never close the popup synchronously
  // here, or it flickers on every keystroke (close now, reopen after fsList)
  // and Enter can race the in-flight list and submit the @draft as a prompt.
  if (text[0] === '@') {
    // But do close a stale slash-command popup when the draft is switched to
    // an @-mention (e.g. select-all then '@'): the old slash items would
    // linger until fsList resolves, and Enter in the window could accept one
    // — turning the draft into a slash command and submitting it. The mention
    // popup is (re)opened by maybeUpdateAtMention when its fsList settles.
    if (s.completionOpen && !s.atMentionOpen) closeCompletion(s);
    return;
  }
  if (text[0] !== '/' || isBashDraft(text)) {
    closeCompletion(s);
    return;
  }
  // This is a slash-command popup, not a mention popup: clear a latched
  // atMentionOpen so maybeUpdateAtMention's no-match branch (it runs after
  // every keystroke, after this reducer) never closes the popup we just
  // opened below.
  s.atMentionOpen = false;

  const whitespaceMatch = text.match(/^\/(\S+)\s+(\S*)$/);
  if (whitespaceMatch) {
    const [, cmdName, argPrefix] = whitespaceMatch;
    const cmd = findSlashCommand(cmdName);
    if (!cmd) {
      closeCompletion(s);
      return;
    }
    const args = argCompletions[cmd.name] ?? [];
    const filtered = fuzzyFilter(args, argPrefix, (a) => a);
    if (filtered.length === 0) {
      closeCompletion(s);
      return;
    }
    s.completionOpen = true;
    s.completionPrefix = text.length - argPrefix.length;
    s.completionItems = filtered.map((a) => ({ value: a, label: a, description: undefined }));
    s.completionSelected = bestMatchIndex(filtered, argPrefix);
    return;
  }

  const needle = text.slice(1);
  if (needle.includes(' ') || needle.includes('/')) {
    closeCompletion(s);
    return;
  }
  const filtered = fuzzyFilter(slashCommands, needle, (c) => c.name);
  if (filtered.length === 0) {
    closeCompletion(s);
    return;
  }
  s.completionOpen = true;
  s.completionPrefix = 0;
  s.completionItems = filtered.map((c) => ({
    value: `/${c.name}`,
    label: `/${c.name}`,
    description: slashCommandDescription(c),
  }));
  s.completionSelected = bestMatchIndex(filtered, needle);
}

function bestMatchIndex(items: (string | { name: string })[], prefix: string): number {
  const p = prefix.toLowerCase();
  for (let i = 0; i < items.length; i++) {
    const name = typeof items[i] === 'string' ? (items[i] as string) : (items[i] as { name: string }).name;
    if (name.toLowerCase() === p) return i;
  }
  for (let i = 0; i < items.length; i++) {
    const name = typeof items[i] === 'string' ? (items[i] as string) : (items[i] as { name: string }).name;
    if (name.toLowerCase().startsWith(p)) return i;
  }
  return 0;
}

export function closeCompletion(s: State): void {
  s.completionOpen = false;
  s.completionItems = [];
  s.completionSelected = 0;
  s.completionPrefix = 0;
  s.atMentionOpen = false;
  s.atMentionPrefix = 0;
}

function syncQuestionTab(s: State): void {
  const q = s.currentQuestion;
  if (!q || !q.allQuestions) return;
  const idx = q.questionTabIndex ?? 0;
  const total = q.allQuestions.length + 1;
  if (idx >= total) return;
  const qq = q.allQuestions[idx];
  if (!qq) return;
  q.itemId = qq.itemId;
  q.question = qq.question;
  q.kind = qq.kind;
  q.options = qq.options;
  q.allowOther = qq.allowOther;
  q.otherLabel = qq.otherLabel;
  s.questionSelectedIndex = 0;
  s.questionOtherText = qq.otherText ?? '';
}
