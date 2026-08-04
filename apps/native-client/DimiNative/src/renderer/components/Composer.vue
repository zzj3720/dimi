<script setup lang="ts">
// Codex-style composer: 25px capsule + completion popup + bottom toolbar.
// Keyboard handling mirrors the old main.js editor bindings.
import { ref, watch, nextTick } from 'vue';
import { state, Msg, isBashDraft, findSlashCommand, APPROVAL_CHOICES } from '../store';
import { dispatch, sendBtw, maybeUpdateAtMention } from '../api';

const inputEl = ref<HTMLTextAreaElement | null>(null);
const completionEl = ref<HTMLElement | null>(null);

// Keep the selected completion row in view (Codex list behavior).
watch(
  () => state.completionSelected,
  () => {
    void nextTick(() => {
      const sel = completionEl.value?.querySelector('.selected');
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
      dispatch(Msg.CompletionClose());
      return;
    }
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
    if (k === 'c' || k === 'd') {
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
    if (k === 's') {
      evt.preventDefault();
      dispatch(Msg.Steer());
      return;
    }
    if (k === 'o') {
      evt.preventDefault();
      dispatch(Msg.ExpandToggle());
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

  // Plain Enter submits.
  if (evt.key === 'Enter' && !evt.shiftKey) {
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

function steerMode(mode: 'steer' | 'queue'): void {
  dispatch(Msg.SetBusyInputMode(mode));
}

function sendBtwText(): void {
  if (state.btwDraft.trim()) sendBtw(state.btwDraft);
}
</script>

<template>
  <footer class="composer">
    <!-- Completion popup -->
    <div v-if="state.completionOpen && state.completionItems.length > 0" ref="completionEl" class="completion">
      <div
        v-for="(item, i) in state.completionItems"
        :key="i"
        class="completion-item"
        :class="{ selected: i === state.completionSelected }"
        @mousedown.prevent="acceptCompletion"
      >
        <span class="pointer">{{ i === state.completionSelected ? '❯ ' : '  ' }}</span>
        <span class="value">{{ item.label }}</span>
        <span v-if="item.description" class="desc">{{ item.description }}</span>
      </div>
    </div>

    <div class="composer-capsule">
      <textarea
        ref="inputEl"
        v-model="state.draft"
        class="input"
        rows="1"
        placeholder="Message…"
        autofocus
        @input="onInput"
        @keydown="onKeydown"
        @compositionend="onInput"
      ></textarea>
      <button
        class="send-btn"
        :disabled="!state.draft.trim() || !state.currentSessionId"
        title="Send"
        @click="dispatch(Msg.Submit())"
      >↑</button>
    </div>

    <div class="composer-toolbar">
      <span class="model-pill" @click="dispatch(Msg.SettingsOpen())">{{ state.modelName || '模型' }}</span>
      <span v-if="state.queued.length > 0" class="queued-count">{{ state.queued.length }} queued</span>
      <template v-if="state.busy">
        <button class="btn btn-ghost" :class="{ 'btn-selected': state.busyInputMode === 'steer' }" @click="steerMode('steer')">steer</button>
        <button class="btn btn-ghost" :class="{ 'btn-selected': state.busyInputMode === 'queue' }" @click="steerMode('queue')">queue</button>
        <button class="btn btn-ghost" @click="dispatch(Msg.Cancel())">Cancel</button>
      </template>
      <span class="hint">{{ state.statusMsg }}</span>
      <span class="spacer"></span>
      <span class="footer-right">{{ state.footerContext }}</span>
    </div>
  </footer>
</template>
