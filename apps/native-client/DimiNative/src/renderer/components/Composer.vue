<script setup lang="ts">
// Codex-style composer: 25px capsule + completion popup + bottom toolbar.
// Keyboard handling mirrors the old main.js editor bindings.
import { ref, watch, nextTick, computed, onMounted } from 'vue';
import { state, Msg, findSlashCommand, APPROVAL_CHOICES } from '../store';
import { dispatch, maybeUpdateAtMention } from '../api';
import { icons } from '../icons';
import {
  composer, capsule, footer, inputRow, inputWrap, composerLeft, composerRight,
  composerBtn, modelPill, modelPillName, modelPillMode, input, sendBtn, composerToolbar, hint, queuedCount,
  completion, completionItem, completionPointer, completionValue, completionDesc, completionSelected,
  btn, btnGhost,
} from './Composer.styles';

// Codex shows a short model name in the composer pill (e.g. "5.6 Terra"),
// not the full provider/model ref.
const shortModelName = computed(() => {
  const m = state.modelName || '';
  if (!m) return '模型';
  const last = m.split('/').pop() ?? m;
  return last.length > 24 ? last.slice(0, 22) + '…' : last;
});

// Icon paths measured from codex (design/04-composer.md §8). Inlined here
// until icons.ts grows `chevron` / `mic` entries — swap to icons.* when added.
const chevronIcon = {
  vb: '0 0 16 16',
  paths: [
    'M12.1338 5.94433C12.3919 5.77382 12.7434 5.80202 12.9707 6.02929C13.1979 6.25656 13.2261 6.60807 13.0556 6.8662L12.9707 6.9707L8.47067 11.4707C8.21097 11.7304 7.78896 11.7304 7.52926 11.4707L3.02926 6.9707L2.9443 6.8662C2.77379 6.60807 2.80199 6.25656 3.02926 6.02929C3.25653 5.80202 3.60804 5.77382 3.86617 5.94433L3.97067 6.02929L7.99996 10.0586L12.0293 6.02929L12.1338 5.94433Z',
  ],
};
const micIcon = {
  vb: '0 0 20 20',
  paths: [
    'M15.7806 10.1963C16.1326 10.3011 16.3336 10.6714 16.2288 11.0234L16.1487 11.2725C15.3429 13.6262 13.2236 15.3697 10.6644 15.6299L10.6653 16.835H12.0833L12.2171 16.8486C12.5202 16.9106 12.7484 17.1786 12.7484 17.5C12.7484 17.8214 12.5202 18.0894 12.2171 18.1514L12.0833 18.165H7.91632C7.5492 18.1649 7.25128 17.8672 7.25128 17.5C7.25128 17.1328 7.5492 16.8351 7.91632 16.835H9.33527L9.33429 15.6299C6.775 15.3697 4.6558 13.6262 3.84992 11.2725L3.76984 11.0234L3.74445 10.8906C3.71751 10.5825 3.91011 10.2879 4.21808 10.1963C4.52615 10.1047 4.84769 10.2466 4.99347 10.5195L5.04523 10.6436L5.10871 10.8418C5.8047 12.8745 7.73211 14.335 9.99933 14.335C12.3396 14.3349 14.3179 12.7789 14.9534 10.6436L15.0052 10.5195C15.151 10.2466 15.4725 10.1046 15.7806 10.1963ZM12.2513 5.41699C12.2513 4.17354 11.2437 3.16521 10.0003 3.16504C8.75675 3.16504 7.74835 4.17343 7.74835 5.41699V9.16699C7.74853 10.4104 8.75685 11.418 10.0003 11.418C11.2436 11.4178 12.2511 10.4103 12.2513 9.16699V5.41699ZM13.5814 9.16699C13.5812 11.1448 11.9781 12.7479 10.0003 12.748C8.02232 12.748 6.41845 11.1449 6.41828 9.16699V5.41699C6.41828 3.43889 8.02221 1.83496 10.0003 1.83496C11.9783 1.83514 13.5814 3.439 13.5814 5.41699V9.16699Z',
  ],
};

const inputEl = ref<HTMLElement | null>(null);
const completionEl = ref<HTMLElement | null>(null);

// Keep the selected completion row in view (Codex list behavior).
watch(
  () => state.completionSelected,
  () => {
    void nextTick(() => {
      const sel = completionEl.value?.querySelector('.' + completionSelected);
      sel?.scrollIntoView({ block: 'nearest' });
    });
  },
);

const isMac = navigator.platform.startsWith('Mac');
function ctrlKey(evt: KeyboardEvent): boolean {
  return isMac ? evt.metaKey : evt.ctrlKey;
}

function onInput(e: Event): void {
  const text = (e.target as HTMLElement).textContent ?? '';
  dispatch(Msg.DraftChange(text));
  void maybeUpdateAtMention(text);
}

function onKeydown(evt: KeyboardEvent): void {
  // IME composition: let the IME own Enter/arrow keys until composition ends.
  if (evt.isComposing) return;

  // Completion popup priority.
  if (state.completionOpen) {
    if (evt.key === 'ArrowDown') {
      evt.preventDefault();
      dispatch(Msg.CompletionMove(1));
      return;
    }
    if (evt.key === 'ArrowUp') {
      evt.preventDefault();
      dispatch(Msg.CompletionMove(-1));
      return;
    }
    if (evt.key === 'Enter' || evt.key === 'Tab') {
      evt.preventDefault();
      const acceptViaTab = evt.key === 'Tab';
      dispatch(Msg.CompletionAccept());
      const draft = state.draft;
      if (draft.startsWith('/') && findSlashCommand(draft.slice(1).split(/\s/)[0])) {
        if (acceptViaTab) {
          dispatch(Msg.DraftChange(draft + ' '));
        } else {
          dispatch(Msg.Submit());
        }
      }
      return;
    }
    if (evt.key === 'Escape') {
      evt.preventDefault();
      // Don't let this bubble to the window handler, which would then also
      // dispatch Msg.Escape() and cancel the turn on top of closing the popup.
      evt.stopPropagation();
      dispatch(Msg.CompletionClose());
      return;
    }
  }

  // Approval / question dialogs: arrow keys move the selected option. Only
  // intercept while a dialog is open — empty-draft history navigation stays
  // with the existing window-level handler.
  if (state.currentApproval && (evt.key === 'ArrowUp' || evt.key === 'ArrowDown')) {
    evt.preventDefault();
    dispatch(Msg.ApprovalMove(evt.key === 'ArrowUp' ? -1 : 1));
    return;
  }
  if (state.currentQuestion && (evt.key === 'ArrowUp' || evt.key === 'ArrowDown')) {
    evt.preventDefault();
    dispatch(Msg.QuestionMove(evt.key === 'ArrowUp' ? -1 : 1));
    return;
  }

  // Question dialog keys.
  if (state.currentQuestion) {
    if (/^[1-9]$/.test(evt.key)) {
      evt.preventDefault();
      const idx = Number(evt.key) - 1;
      const q = state.currentQuestion;
      if (idx < (q.options ?? []).length) {
        if (q.kind === 'multi' || q.kind === 'multi_with_other') {
          dispatch({ type: 'question_toggle', index: idx });
        } else {
          dispatch({ type: 'question_select', index: idx });
          dispatch(Msg.QuestionTab(1));
        }
      }
      return;
    }
    if (evt.key === ' ') {
      evt.preventDefault();
      dispatch({ type: 'question_toggle', index: state.questionSelectedIndex });
      return;
    }
    if (evt.key === 'ArrowLeft') {
      evt.preventDefault();
      dispatch(Msg.QuestionTab(-1));
      return;
    }
    if (evt.key === 'ArrowRight' || evt.key === 'Tab') {
      evt.preventDefault();
      dispatch(Msg.QuestionTab(1));
      return;
    }
  }

  // Approval dialog keys.
  if (state.currentApproval) {
    if (/^[1-9]$/.test(evt.key)) {
      evt.preventDefault();
      const idx = Number(evt.key) - 1;
      if (idx < APPROVAL_CHOICES.length) {
        dispatch(Msg.ApprovalSelect(idx));
        if (idx === 3) {
          state.approvalFeedbackMode = true;
        } else {
          dispatch(Msg.ApprovalConfirm());
        }
      }
      return;
    }
  }

  // Ctrl / Cmd combos.
  if (ctrlKey(evt)) {
    const k = evt.key.toLowerCase();
    if (k === 'c') {
      // Let Cmd/Ctrl+C copy when there is a text selection; otherwise cancel.
      const sel = window.getSelection();
      const hasSelection = sel && !sel.isCollapsed && sel.toString().length > 0;
      if (hasSelection) return;
      evt.preventDefault();
      if (state.currentApproval) {
        dispatch(Msg.ApprovalReject());
        return;
      }
      if (state.currentQuestion) {
        dispatch({ type: 'question_dismiss' });
        return;
      }
      dispatch(Msg.Cancel());
      return;
    }
    if (k === 'd') {
      evt.preventDefault();
      if (state.currentApproval) {
        dispatch(Msg.ApprovalReject());
        return;
      }
      if (state.currentQuestion) {
        dispatch({ type: 'question_dismiss' });
        return;
      }
      dispatch(Msg.Cancel());
      return;
    }
    if (k === 't') {
      evt.preventDefault();
      dispatch({ type: 'todo_toggle' });
      return;
    }
    if (k === '-') {
      evt.preventDefault();
      dispatch({ type: 'undo' });
      return;
    }
  }

  // Enter: approval / question dialogs take priority over submitting.
  if (evt.key === 'Enter' && !evt.shiftKey) {
    if (state.currentApproval) {
      evt.preventDefault();
      dispatch(Msg.ApprovalConfirm());
      return;
    }
    if (state.currentQuestion) {
      const q = state.currentQuestion;
      const idx = state.questionSelectedIndex;
      evt.preventDefault();
      if (q.kind === 'multi' || q.kind === 'multi_with_other') {
        dispatch({ type: 'question_toggle', index: idx });
      } else if ((q.options ?? []).length > 0) {
        dispatch({ type: 'question_select', index: idx });
        dispatch(Msg.QuestionTab(1));
      }
      return;
    }
    evt.preventDefault();
    dispatch(Msg.Submit());
    return;
  }
}

// Keep the editable div content in sync (IME-safe); don't touch the DOM when
// they already match so the caret position is preserved.
watch(
  () => state.draft,
  (v) => {
    const el = inputEl.value;
    if (el && (el.textContent ?? '') !== v) el.textContent = v;
  },
);

onMounted(() => {
  void nextTick(() => inputEl.value?.focus());
});

function acceptCompletion(): void {
  dispatch(Msg.CompletionAccept());
}

function onSend(): void {
  // The send button must not bypass an open completion popup: clicking it
  // while a popup is showing (slash command or @-mention) would submit the
  // raw @draft or partial command. Mirror the keyboard Enter branch — accept
  // the highlighted item first, and submit afterwards only when the accepted
  // draft is a complete slash command.
  if (state.completionOpen && state.completionItems.length > 0) {
    dispatch(Msg.CompletionAccept());
    const draft = state.draft;
    if (draft.startsWith('/') && findSlashCommand(draft.slice(1).split(/\s/)[0])) {
      dispatch(Msg.Submit());
    }
    return;
  }
  dispatch(Msg.Submit());
}

function steerMode(mode: 'steer' | 'queue'): void {
  dispatch(Msg.SetBusyInputMode(mode));
}
</script>

<template>
  <footer :class="composer">
    <!-- Completion popup -->
    <div v-if="state.completionOpen && state.completionItems.length > 0" ref="completionEl" :class="completion" data-testid="completion">
      <div
        v-for="(item, i) in state.completionItems"
        :key="i"
        :class="[completionItem, { [completionSelected]: i === state.completionSelected }]"
        data-testid="completion-item"
        @mousedown.prevent="acceptCompletion"
        @mouseenter="state.completionSelected = i"
      >
        <span :class="completionPointer">{{ i === state.completionSelected ? '❯ ' : '  ' }}</span>
        <span :class="completionValue" data-testid="completion-value">{{ item.label }}</span>
        <span v-if="item.description" :class="completionDesc">{{ item.description }}</span>
      </div>
    </div>

    <div :class="capsule">
      <div :class="footer">
        <div :class="inputRow">
          <div :class="inputWrap">
            <div
              ref="inputEl"
              :class="input"
              contenteditable="true"
              data-placeholder="Message…"
              role="textbox"
              aria-multiline="true"
              data-testid="composer-input"
              @input="onInput"
              @keydown="onKeydown"
              @compositionend="onInput"
            ></div>
          </div>
        </div>
        <div :class="composerLeft">
          <!-- Codex left button: 添加文件等内容 (plus). dimi has no attach UI,
               so it reports the same "not implemented" status as before. -->
          <button :class="composerBtn" aria-label="添加文件等内容" @click="state.statusMsg = '附件（暂未实现）'">
            <svg :viewBox="icons.plus.vb" fill="currentColor" aria-hidden="true"><path v-for="(p, i) in icons.plus.paths" :key="i" :d="p" /></svg>
          </button>
        </div>
        <div :class="composerRight">
          <!-- Codex model pill: short model name (#fff) + mode (tertiary) + chevron 14×14.
               dimi has no model picker — clicking still opens settings. -->
          <button :class="modelPill" @click="dispatch(Msg.SettingsOpen())">
            <span :class="modelPillName">{{ shortModelName }}</span>
            <span :class="modelPillMode">轻度</span>
            <svg :viewBox="chevronIcon.vb" fill="currentColor" aria-hidden="true"><path v-for="(p, i) in chevronIcon.paths" :key="i" :d="p" /></svg>
          </button>
          <button :class="composerBtn" aria-label="听写" @click="state.statusMsg = '听写（暂未实现）'">
            <svg :viewBox="micIcon.vb" fill="currentColor" aria-hidden="true"><path v-for="(p, i) in micIcon.paths" :key="i" :d="p" /></svg>
          </button>
          <button
            :class="sendBtn"
            :disabled="!state.draft.trim() || !state.currentSessionId"
            aria-label="发送"
            data-testid="send-btn"
            @click="onSend"
          >
            <svg :viewBox="icons.send.vb" fill="currentColor" aria-hidden="true"><path v-for="(p, i) in icons.send.paths" :key="i" :d="p" /></svg>
          </button>
        </div>
      </div>
    </div>

    <!-- Codex keeps model/mode info inside the capsule pill, no extra row -->
    

    <div v-if="state.busy || state.statusMsg || state.queued.length > 0" :class="composerToolbar">
      <span v-if="state.queued.length > 0" :class="queuedCount">{{ state.queued.length }} queued</span>
      <template v-if="state.busy">
        <button :class="[btn, btnGhost, { 'btn-selected': state.busyInputMode === 'steer' }]" @click="steerMode('steer')">steer</button>
        <button :class="[btn, btnGhost, { 'btn-selected': state.busyInputMode === 'queue' }]" @click="steerMode('queue')">queue</button>
        <button :class="[btn, btnGhost]" @click="dispatch(Msg.Cancel())">Cancel</button>
      </template>
      <span :class="hint" data-testid="status-msg">{{ state.statusMsg }}</span>
    </div>
  </footer>
</template>
