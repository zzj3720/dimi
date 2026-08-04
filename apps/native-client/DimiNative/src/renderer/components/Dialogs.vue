<script setup lang="ts">
// Modal dialogs: session picker, settings, help, BTW, approval, question.
import { computed, nextTick, ref } from 'vue';
import { state, Msg, filteredSessions, slashCommands, APPROVAL_CHOICES } from '../store';
import type { PermissionMode, Question } from '../store';
import { api, dispatch, loadSessions, loadMoreSessions, sendBtw } from '../api';
import { icons } from '../icons';
import {
  dialogRoot, dialogBackdrop, dialog, dialogPicker, dialogApproval, dialogTitle, dialogBody, dialogClose,
  searchInput, listItem, listItemIcon, listItemTitle, listItemHint, listItemSub, listItemSelected, toolName,
  btn, btnGhost, btnPrimary, badge, badgePrimary, badgeOutline, bodyText,
} from './Dialogs.styles';

// ---- session picker
const pickerList = computed(() => filteredSessions(state));
function pickerKeydown(e: KeyboardEvent): void {
  if (e.key === 'ArrowDown') {
    e.preventDefault();
    dispatch(Msg.PickerMove(1));
  } else if (e.key === 'ArrowUp') {
    e.preventDefault();
    dispatch(Msg.PickerMove(-1));
  } else if (e.key === 'Enter') {
    e.preventDefault();
    dispatch(Msg.PickerSelect());
  } else if (e.key === 'Escape') {
    e.preventDefault();
    dispatch(Msg.Escape());
  }
}

function onPickerScroll(e: Event): void {
  const el = e.target as HTMLElement;
  if (el.scrollTop + el.clientHeight >= el.scrollHeight - 40) void loadMoreSessions();
}

// ---- settings
const models = ref<{ value: string; label: string }[]>([]);
let modelsLoaded = false;
const feedbackInput = ref<HTMLInputElement | null>(null);
async function openSettings(): Promise<void> {
  if (modelsLoaded) return;
  modelsLoaded = true;
  try {
    const data = await window.dimi!.request({ method: 'GET', url: '/api/v1/models' });
    const items = ((data.json as { data?: { items?: { provider: string; model: string; display_name?: string }[] } })?.data?.items) ?? [];
    models.value = items.map((m) => ({
      value: `${m.provider}/${m.model}`,
      label: `${m.display_name ?? m.model} (${m.provider}/${m.model})`,
    }));
  } catch {
    models.value = [];
  }
}

// The Settings selects call the server directly (mirrors the /model and
// /permission slash commands in api.ts) — the legacy `settings_set_*`
// dimi:msg events were never consumed anywhere.
async function setDefaultModel(ref: string): Promise<void> {
  if (!ref) return;
  try {
    const data = await api('POST', `/api/v1/models/${encodeURIComponent(ref)}:set_default`, {});
    state.statusMsg = `default model → ${data?.data?.default_model ?? ref}`;
  } catch (e) {
    state.statusMsg = `model set failed: ${(e as Error).message}`;
  }
}

async function setPermissionMode(mode: string): Promise<void> {
  try {
    await api('POST', '/api/v1/config', { default_permission_mode: mode });
    state.permissionMode = mode as PermissionMode;
    state.statusMsg = `permission mode → ${mode}`;
  } catch (e) {
    state.statusMsg = `permission failed: ${(e as Error).message}`;
  }
}

// Approval row click: choices 0-2 select + confirm immediately; choice 3
// (Reject with feedback) only enters feedback mode on first click and
// confirms on a second click, so an empty feedback can't be submitted
// accidentally (Enter in the feedback input also confirms).
function onApprovalChoice(i: number): void {
  if (i === 3) {
    if (state.approvalSelectedIndex === i) {
      // Second click on "Reject with feedback…" confirms, but never with an
      // empty feedback — keep the dialog in feedback mode and focus the input.
      if (!state.approvalFeedbackText.trim()) {
        // Esc may have left the selection on row 3 with the mode flag cleared;
        // re-enter feedback mode (matching the first-click path) so Enter in
        // the visible feedback input goes through the guarded feedback branch
        // instead of a plain 'approving…' → feedback-less rejection.
        state.approvalFeedbackMode = true;
        void nextTick(() => feedbackInput.value?.focus());
        return;
      }
      dispatch(Msg.ApprovalConfirm());
    } else {
      dispatch(Msg.ApprovalSelect(i));
      // Match the keyboard digit-4 path so the statusMsg text and Esc
      // behavior (exit feedback mode instead of rejecting) stay consistent.
      state.approvalFeedbackMode = true;
      void nextTick(() => feedbackInput.value?.focus());
    }
    return;
  }
  dispatch(Msg.ApprovalSelect(i));
  dispatch(Msg.ApprovalConfirm());
}

// ---- question helpers
function hasAnswer(q: Question): boolean {
  return (q.options ?? []).some((o) => o.selected) || (q.otherText && q.otherText.trim().length > 0);
}
function questionTabs(q: Question): number {
  return (q.allQuestions?.length ?? 1) + 1;
}

// ---- btw
function btwSend(): void {
  if (state.btwDraft.trim()) sendBtw(state.btwDraft);
}
</script>

<template>
  <div :class="dialogRoot">
    <!-- Session picker: compact command menu -->
    <div v-if="state.pickerOpen" :class="dialogBackdrop" @mousedown.self="dispatch(Msg.PickerClose())">
      <div :class="dialogPicker">
        <input :class="searchInput" placeholder="Search sessions…" :value="state.pickerQuery" @input="dispatch(Msg.PickerSearch(($event.target as HTMLInputElement).value))" @keydown="pickerKeydown" />
        <div style="overflow-y: auto; max-height: 300px; margin-top: 6px">
          <div
            v-for="(s, i) in pickerList"
            :key="s.id"
            :class="[listItem, { [listItemSelected]: i === state.pickerSelectedIndex }]"
            @mousedown.prevent="dispatch(Msg.PickerSelect())"
          >
            <span :class="listItemIcon">
              <svg v-if="i === state.pickerSelectedIndex" :viewBox="icons.radio.vb" fill="currentColor" aria-hidden="true"><path :d="icons.radio.paths[0]" /></svg>
            </span>
            <span :class="listItemTitle">{{ s.title || '(untitled)' }}</span>
            <span :class="listItemHint">{{ s.last_prompt || s.cwd || '' }}</span>
          </div>
          <div v-if="pickerList.length === 0" :class="[listItem, listItemSub]">{{ state.sessionsLoading ? 'Loading…' : 'No sessions found.' }}</div>
        </div>
        <div :class="listItemSub" style="margin-top: 6px; padding: 4px 8px">↑↓ navigate · Enter select · Esc cancel</div>
      </div>
    </div>

    <!-- Settings -->
    <div v-if="state.settingsDialogOpen" :class="dialogBackdrop" @mousedown.self="dispatch(Msg.SettingsClose())">
      <div :class="dialog" @click="openSettings">
        <button :class="dialogClose" aria-label="Close" @click="dispatch(Msg.SettingsClose())">
          <svg :viewBox="icons.close.vb" fill="currentColor" aria-hidden="true"><path :d="icons.close.paths[0]" /></svg>
        </button>
        <div :class="dialogBody">
          <div :class="dialogTitle" style="margin-bottom: 16px">Settings</div>
          <div style="display: flex; align-items: center; gap: 8px; margin-bottom: 10px">
            <span :class="listItemSub" style="min-width: 110px">Default model</span>
            <select :class="searchInput" :value="state.modelName" @change="setDefaultModel(($event.target as HTMLSelectElement).value)">
              <option v-if="models.length === 0" value="">loading…</option>
              <option v-for="m in models" :key="m.value" :value="m.value">{{ m.label }}</option>
            </select>
          </div>
          <div style="display: flex; align-items: center; gap: 8px; margin-bottom: 10px">
            <span :class="listItemSub" style="min-width: 110px">Permission mode</span>
            <select :class="searchInput" :value="state.permissionMode" @change="setPermissionMode(($event.target as HTMLSelectElement).value)">
              <option value="manual">manual</option>
              <option value="auto">auto</option>
              <option value="yolo">yolo</option>
            </select>
          </div>
          <div style="display: flex; align-items: center; gap: 8px">
            <span :class="listItemSub" style="min-width: 110px">Plan mode</span>
            <button :class="[btn, btnGhost]" @click="dispatch({ type: 'plan_mode_toggle' })">{{ state.planMode ? 'on (toggle)' : 'off (toggle)' }}</button>
          </div>
          <div style="display: flex; gap: 8px; justify-content: flex-end; margin-top: 16px">
            <button :class="[btn, btnGhost]" @click="dispatch(Msg.SettingsClose())">Close</button>
          </div>
        </div>
      </div>
    </div>

    <!-- Help -->
    <div v-if="state.helpDialogOpen" :class="dialogBackdrop" @mousedown.self="dispatch(Msg.Escape())">
      <div :class="dialog">
        <button :class="dialogClose" aria-label="Close" @click="dispatch(Msg.Escape())">
          <svg :viewBox="icons.close.vb" fill="currentColor" aria-hidden="true"><path :d="icons.close.paths[0]" /></svg>
        </button>
        <div :class="dialogBody" style="max-height: 420px">
          <div :class="dialogTitle" style="margin-bottom: 12px">Help</div>
          <div v-for="c in slashCommands" :key="c.name" :class="listItem">
            <span :class="toolName">{{ '/' + c.name }}</span>
            <span :class="listItemSub" style="flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap">{{ c.hint }} — {{ c.desc }}</span>
          </div>
          <div style="display: flex; gap: 8px; justify-content: flex-end; margin-top: 16px">
            <button :class="[btn, btnGhost]" @click="dispatch(Msg.Escape())">Close</button>
          </div>
        </div>
      </div>
    </div>

    <!-- BTW -->
    <div v-if="state.btwOpen" :class="dialogBackdrop" @mousedown.self="state.btwOpen = false">
      <div :class="dialog">
        <button :class="dialogClose" aria-label="Close" @click="dispatch(Msg.Escape())">
          <svg :viewBox="icons.close.vb" fill="currentColor" aria-hidden="true"><path :d="icons.close.paths[0]" /></svg>
        </button>
        <div :class="dialogBody">
          <div :class="dialogTitle" style="margin-bottom: 12px">BTW</div>
          <div v-if="state.btwPrompt" :class="listItem">
            <div :class="bodyText">{{ state.btwPrompt }}</div>
          </div>
          <div v-if="state.btwAnswer" :class="listItem">
            <div :class="bodyText">{{ state.btwAnswer }}</div>
          </div>
          <div v-else-if="state.btwBusy" :class="listItemSub">…</div>
          <input :class="searchInput" placeholder="Ask by the way…" :value="state.btwDraft" @keydown.enter="btwSend" @keydown.esc.prevent="dispatch(Msg.Escape())" @input="state.btwDraft = ($event.target as HTMLInputElement).value" />
          <div :class="listItemSub" style="margin-top: 8px">Enter to ask · Esc to close</div>
        </div>
      </div>
    </div>

    <!-- Approval: permission prompt card -->
    <div v-if="state.currentApproval" :class="dialogBackdrop">
      <div :class="dialogApproval">
        <button :class="dialogClose" aria-label="Reject" @click="dispatch(Msg.Escape())">
          <svg :viewBox="icons.close.vb" fill="currentColor" aria-hidden="true"><path :d="icons.close.paths[0]" /></svg>
        </button>
        <div :class="dialogBody">
          <div :class="dialogTitle" style="margin-bottom: 12px">{{ state.currentApproval.toolName || 'Approval required' }}</div>
          <div :class="bodyText" style="font-family: ui-monospace, 'SF Mono', Menlo, monospace">{{ state.currentApproval.action }}</div>
          <pre v-if="state.currentApproval.command" :class="bodyText" style="font-family: ui-monospace, 'SF Mono', Menlo, monospace; white-space: pre-wrap; word-break: break-word; background: #141414; padding: 6px 8px; border: 1px solid rgba(255, 255, 255, 0.08); border-radius: 8px; max-height: 72px; overflow: auto">{{ state.currentApproval.command }}</pre>
          <pre v-if="state.approvalPreview && state.currentApproval.command" :class="bodyText" style="font-family: ui-monospace, 'SF Mono', Menlo, monospace; white-space: pre-wrap; word-break: break-word; background: #141414; padding: 6px 8px; border: 1px solid rgba(255, 255, 255, 0.08); border-radius: 8px; max-height: 320px; overflow: auto">{{ state.currentApproval.command }}</pre>
          <div style="margin-top: 8px">
            <div
              v-for="(opt, i) in APPROVAL_CHOICES"
              :key="i"
              :class="[listItem, { [listItemSelected]: i === state.approvalSelectedIndex }]"
              @mousedown.prevent="onApprovalChoice(i)"
            >{{ opt.label }}</div>
          </div>
          <input
            v-if="state.approvalSelectedIndex === 3"
            ref="feedbackInput"
            :class="searchInput"
            placeholder="Feedback…"
            :value="state.approvalFeedbackText"
            @input="state.approvalFeedbackText = ($event.target as HTMLInputElement).value"
            @keydown.enter="dispatch(Msg.ApprovalConfirm())"
          />
          <div :class="listItemSub" style="margin-top: 8px">↑↓ navigate · Enter confirm · 1-4 select · Esc reject</div>
        </div>
      </div>
    </div>

    <!-- Question -->
    <div v-if="state.currentQuestion" :class="dialogBackdrop">
      <div :class="dialog">
        <button :class="dialogClose" aria-label="Close" @click="dispatch(Msg.QuestionDismiss())">
          <svg :viewBox="icons.close.vb" fill="currentColor" aria-hidden="true"><path :d="icons.close.paths[0]" /></svg>
        </button>
        <div :class="dialogBody">
          <div :class="dialogTitle" style="margin-bottom: 12px">Question</div>
          <div style="display: flex; gap: 4px; flex-wrap: wrap; margin-bottom: 8px">
            <span
              v-for="(qq, i) in (state.currentQuestion.allQuestions ?? [state.currentQuestion])"
              :key="i"
              :class="[badge, i === (state.currentQuestion.questionTabIndex ?? 0) ? badgePrimary : badgeOutline]"
            >Q{{ i + 1 }}{{ hasAnswer(qq) ? ' ✓' : '' }}</span>
            <span :class="[badge, (state.currentQuestion.questionTabIndex ?? 0) === questionTabs(state.currentQuestion) - 1 ? badgePrimary : badgeOutline]">Submit</span>
          </div>
          <div :class="listItemTitle" style="font-weight: 600">{{ state.currentQuestion.question }}</div>
          <div style="margin-top: 8px">
            <div
              v-for="(opt, i) in (state.currentQuestion.options ?? [])"
              :key="i"
              :class="[listItem, { [listItemSelected]: i === state.questionSelectedIndex }]"
              @mousedown.prevent="state.currentQuestion.kind === 'multi' ? dispatch({ type: 'question_toggle', index: i }) : (dispatch({ type: 'question_select', index: i }), dispatch(Msg.QuestionConfirm()))"
            >{{ (state.currentQuestion.kind === 'multi' || state.currentQuestion.kind === 'multi_with_other') ? (opt.selected ? '✓ ' : '○ ') : (opt.selected ? '● ' : '○ ') }}{{ opt.label }}</div>
          </div>
          <input
            v-if="state.currentQuestion.allowOther"
            :class="searchInput"
            :placeholder="state.currentQuestion.otherLabel || 'Other…'"
            :value="state.questionOtherText"
            @input="dispatch({ type: 'question_other', text: ($event.target as HTMLInputElement).value })"
          />
          <div :class="listItemSub" style="margin-top: 8px">←/→ tabs · 1-9 select · space toggle · Enter confirm</div>
          <div style="display: flex; gap: 8px; justify-content: flex-end; margin-top: 16px">
            <button :class="[btn, btnPrimary]" @click="dispatch(Msg.QuestionConfirm())">Submit</button>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>
