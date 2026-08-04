<script setup lang="ts">
// Root shell: Codex-style sidebar + header + transcript + composer + dialogs.
// Owns global keyboard (window keydown) and the dimi:msg event bridge.
import { onMounted, onUnmounted } from 'vue';
import Sidebar from './components/Sidebar.vue';
import HeaderBar from './components/HeaderBar.vue';
import Transcript from './components/Transcript.vue';
import Composer from './components/Composer.vue';
import Dialogs from './components/Dialogs.vue';
import { state, Msg } from './store';
import { dispatch, createSession, detachCurrentTask, recallLastQueued } from './api';
import { app } from './styles/global';
import { shell, mainCol, rootScroll } from './App.styles';

const isMac = navigator.platform.startsWith('Mac');
function ctrlKey(evt: KeyboardEvent): boolean {
  return isMac ? evt.metaKey : evt.ctrlKey;
}

// Global keydown (mirrors the old main.js window handler): Tab/Enter/Escape
// and arrow navigation take priority over the textarea's native behavior.
function onWindowKeydown(evt: KeyboardEvent): void {
  // Ctrl / Cmd combos.
  if (ctrlKey(evt)) {
    const k = evt.key.toLowerCase();
    if (k === 'b') {
      if (state.busy && state.phase !== 'compacting') {
        evt.preventDefault();
        detachCurrentTask();
      }
      return;
    }
    if (k === 'e') {
      if (state.currentApproval) {
        evt.preventDefault();
        // approvalPreview toggles the preview/detail view inside the approval
        // dialog (rendered by Dialogs).
        state.approvalPreview = !state.approvalPreview;
      }
      return;
    }
    if (k === 'a') {
      if (state.pickerOpen) {
        evt.preventDefault();
        dispatch(Msg.PickerScope(state.pickerScope === 'cwd' ? 'all' : 'cwd'));
      }
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
    return;
  }

  // Ignore when typing in an input/textarea (except global navigation keys).
  const tag = (evt.target as HTMLElement)?.tagName;
  const typing = tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT';

  switch (evt.key) {
    case 'Enter':
      if (!evt.shiftKey && !typing) {
        evt.preventDefault();
        if (state.currentApproval) {
          dispatch(Msg.ApprovalConfirm());
        } else if (state.currentQuestion) {
          const q = state.currentQuestion;
          const idx = state.questionSelectedIndex;
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
      if (!typing) {
        if (evt.shiftKey) {
          evt.preventDefault();
          dispatch({ type: 'plan_mode_toggle' });
        } else if (state.completionOpen) {
          evt.preventDefault();
          dispatch(Msg.CompletionAccept());
        }
        // Otherwise let Tab drive native focus navigation.
      }
      return;

    case 'Escape':
      // Element-level handlers (composer / dialogs) preventDefault on Esc;
      // skip so one keypress doesn't reach the reducer twice.
      if (evt.defaultPrevented) return;
      evt.preventDefault();
      dispatch(Msg.Escape());
      return;

    case 'ArrowUp':
      if (!typing) {
        evt.preventDefault();
        if (state.completionOpen) {
          dispatch(Msg.CompletionMove(-1));
          return;
        }
        if (state.pickerOpen) {
          dispatch(Msg.PickerMove(-1));
          return;
        }
        if (state.currentApproval) {
          dispatch(Msg.ApprovalMove(-1));
          return;
        }
        if (state.currentQuestion) {
          dispatch(Msg.QuestionMove(-1));
          return;
        }
        if (state.queued.length > 0 && state.draft.trim() === '') {
          recallLastQueued();
          return;
        }
        dispatch(Msg.HistoryPrev());
      }
      return;

    case 'ArrowDown':
      if (!typing) {
        evt.preventDefault();
        if (state.completionOpen) {
          dispatch(Msg.CompletionMove(1));
          return;
        }
        if (state.pickerOpen) {
          dispatch(Msg.PickerMove(1));
          return;
        }
        if (state.currentApproval) {
          dispatch(Msg.ApprovalMove(1));
          return;
        }
        if (state.currentQuestion) {
          dispatch(Msg.QuestionMove(1));
          return;
        }
        dispatch(Msg.HistoryNext());
      }
      return;

    default:
      return;
  }
}

// dimi:msg bridge from components (new chat, suggestion cards).
function onDimiMsg(evt: Event): void {
  const msg = (evt as CustomEvent).detail;
  if (!msg || typeof msg.type !== 'string') return;
  if (msg.type === 'new_chat') {
    void createSession().then((id) => {
      if (id) dispatch(Msg.SessionSelected(id));
    });
    return;
  }
  if (msg.type === 'suggestion_send') {
    if (state.currentSessionId) {
      state.draft = msg.text;
      dispatch(Msg.Submit());
    } else {
      state.statusMsg = 'select a session first';
    }
    return;
  }
}

const tick = setInterval(() => dispatch(Msg.Tick()), 1000);

onMounted(() => {
  window.addEventListener('keydown', onWindowKeydown);
  window.addEventListener('dimi:msg', onDimiMsg);
});
onUnmounted(() => {
  clearInterval(tick);
  window.removeEventListener('keydown', onWindowKeydown);
  window.removeEventListener('dimi:msg', onDimiMsg);
});
</script>

<template>
  <div :class="[app, rootScroll]">
    <HeaderBar />
    <div :class="shell">
      <Sidebar v-if="state.sidebarVisible" />
      <div :class="mainCol">
        <Transcript />
        <Composer />
      </div>
    </div>
    <Dialogs />
  </div>
</template>
