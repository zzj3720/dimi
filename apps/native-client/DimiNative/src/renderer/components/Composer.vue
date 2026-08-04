<script setup lang="ts">
// Codex-style composer: 25px capsule + completion popup + bottom toolbar.
// Keyboard handling mirrors the old main.js editor bindings.
import { ref, watch, nextTick, computed } from 'vue';
import { state, Msg, findSlashCommand, APPROVAL_CHOICES } from '../store';
import { dispatch, maybeUpdateAtMention } from '../api';
import {
  composer, capsule, footer, inputRow, inputWrap, composerLeft, composerRight,
  composerBtn, modelPill, input, sendBtn, composerToolbar, hint, footerRight, queuedCount,
  completion, completionItem, completionPointer, completionValue, completionDesc, completionSelected,
  btn, btnGhost,
} from './Composer.styles';
import { spacer } from '../styles/global';

// Codex shows a short model name in the composer pill (e.g. "5.6 Terra"),
// not the full provider/model ref.
const shortModelName = computed(() => {
  const m = state.modelName || '';
  if (!m) return '模型';
  const last = m.split('/').pop() ?? m;
  return last.length > 24 ? last.slice(0, 22) + '…' : last;
});

const inputEl = ref<HTMLTextAreaElement | null>(null);
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
  const text = (e.target as HTMLTextAreaElement).value;
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
      const ta = evt.target as HTMLTextAreaElement;
      const noSelection = ta.selectionStart === ta.selectionEnd;
      if (!noSelection) return;
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

// Keep textarea value in sync (IME-safe) and re-run @mention on composition end.
watch(
  () => state.draft,
  (v) => {
    if (inputEl.value && inputEl.value.value !== v) inputEl.value.value = v;
  },
);

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
            <textarea
              ref="inputEl"
              v-model="state.draft"
              :class="input"
              rows="1"
              placeholder="Message…"
              autofocus
              data-testid="composer-input"
              @input="onInput"
              @keydown="onKeydown"
              @compositionend="onInput"
            ></textarea>
          </div>
        </div>
        <div :class="composerLeft">
          <button :class="composerBtn" title="Settings" @click="dispatch(Msg.SettingsOpen())">⚙</button>
        </div>
        <div :class="composerRight">
          <span :class="modelPill" @click="dispatch(Msg.SettingsOpen())">{{ shortModelName }}</span>
          <button
            :class="sendBtn"
            :disabled="!state.draft.trim() || !state.currentSessionId"
            title="Send"
            data-testid="send-btn"
            @click="onSend"
          >↑</button>
        </div>
      </div>
    </div>

    <div v-if="state.busy || state.statusMsg || state.queued.length > 0 || state.footerContext" :class="composerToolbar">
      <span v-if="state.queued.length > 0" :class="queuedCount">{{ state.queued.length }} queued</span>
      <template v-if="state.busy">
        <button :class="[btn, btnGhost, { 'btn-selected': state.busyInputMode === 'steer' }]" @click="steerMode('steer')">steer</button>
        <button :class="[btn, btnGhost, { 'btn-selected': state.busyInputMode === 'queue' }]" @click="steerMode('queue')">queue</button>
        <button :class="[btn, btnGhost]" @click="dispatch(Msg.Cancel())">Cancel</button>
      </template>
      <span :class="hint" data-testid="status-msg">{{ state.statusMsg }}</span>
      <span :class="spacer"></span>
      <span :class="footerRight">{{ state.footerContext }}</span>
    </div>
  </footer>
</template>
