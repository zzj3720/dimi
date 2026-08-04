<script setup lang="ts">
// Modal dialogs: session picker, settings, help, BTW, approval, question.
import { computed, ref } from 'vue';
import { state, Msg, filteredSessions, slashCommands, APPROVAL_CHOICES } from '../store';
import type { Question } from '../store';
import { dispatch, loadSessions, loadMoreSessions, sendBtw } from '../api';
import {
  dialogRoot, dialogBackdrop, dialog, dialogTitle, dialogBody, dialogFooter,
  searchInput, listItem, listItemSelected, listItemTitle, listItemSub, toolName,
  btn, btnGhost, btnPrimary, badge, badgePrimary, badgeOutline, badgeSecondary,
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
    <!-- Session picker -->
    <div v-if="state.pickerOpen" :class="dialogBackdrop" @mousedown.self="dispatch(Msg.PickerClose())">
      <div :class="dialog">
        <div :class="dialogTitle">Sessions</div>
        <div :class="dialogBody">
          <input :class="searchInput" placeholder="Search sessions…" :value="state.pickerQuery" @input="dispatch(Msg.PickerSearch(($event.target as HTMLInputElement).value))" @keydown="pickerKeydown" />
          <div style="overflow-y: auto; max-height: 320px" @scroll="onPickerScroll">
            <div
              v-for="(s, i) in pickerList"
              :key="s.id"
              :class="[listItem, { [listItemSelected]: i === state.pickerSelectedIndex }]"
              @mousedown.prevent="dispatch(Msg.PickerSelect())"
            >
              <div :class="listItemTitle">{{ s.title || '(untitled)' }}</div>
              <div :class="listItemSub">{{ s.id }}</div>
            </div>
            <div v-if="pickerList.length === 0" :class="[listItem, listItemSub]">{{ state.sessionsLoading ? 'Loading…' : 'No sessions found.' }}</div>
          </div>
          <div :class="listItemSub" style="margin-top: 8px">↑↓ navigate · Enter select · Esc cancel</div>
        </div>
        <div :class="dialogFooter">
          <button :class="[btn, btnGhost]" @click="dispatch(Msg.PickerClose())">Close</button>
        </div>
      </div>
    </div>

    <!-- Settings -->
    <div v-if="state.settingsDialogOpen" :class="dialogBackdrop" @mousedown.self="dispatch(Msg.SettingsClose())">
      <div :class="dialog" @click="openSettings">
        <div :class="dialogTitle">Settings</div>
        <div :class="dialogBody">
          <div style="display: flex; align-items: center; gap: 8px; margin-bottom: 10px">
            <span :class="listItemSub" style="min-width: 110px">Default model</span>
            <select :class="searchInput" @change="window.dispatchEvent(new CustomEvent('dimi:msg', { detail: { type: 'settings_set_model', ref: ($event.target as HTMLSelectElement).value } }))">
              <option v-if="models.length === 0">loading…</option>
              <option v-for="m in models" :key="m.value" :value="m.value">{{ m.label }}</option>
            </select>
          </div>
          <div style="display: flex; align-items: center; gap: 8px; margin-bottom: 10px">
            <span :class="listItemSub" style="min-width: 110px">Permission mode</span>
            <select :class="searchInput" :value="state.permissionMode" @change="window.dispatchEvent(new CustomEvent('dimi:msg', { detail: { type: 'settings_set_permission', mode: ($event.target as HTMLSelectElement).value } }))">
              <option value="manual">manual</option>
              <option value="auto">auto</option>
              <option value="yolo">yolo</option>
            </select>
          </div>
          <div style="display: flex; align-items: center; gap: 8px">
            <span :class="listItemSub" style="min-width: 110px">Plan mode</span>
            <button :class="[btn, btnGhost]" @click="dispatch({ type: 'plan_mode_toggle' })">{{ state.planMode ? 'on (toggle)' : 'off (toggle)' }}</button>
          </div>
        </div>
        <div :class="dialogFooter">
          <button :class="[btn, btnGhost]" @click="dispatch(Msg.SettingsClose())">Close</button>
        </div>
      </div>
    </div>

    <!-- Help -->
    <div v-if="state.helpDialogOpen" :class="dialogBackdrop" @mousedown.self="dispatch(Msg.Escape())">
      <div :class="dialog">
        <div :class="dialogTitle">Help</div>
        <div :class="dialogBody" style="max-height: 420px">
          <div v-for="c in slashCommands" :key="c.name" :class="listItem" style="padding: 4px 8px">
            <span><span :class="toolName">{{ '/' + c.name }}</span> <span :class="listItemSub">{{ c.hint }} — {{ c.desc }}</span></span>
          </div>
        </div>
        <div :class="dialogFooter">
          <button :class="[btn, btnGhost]" @click="dispatch(Msg.Escape())">Close</button>
        </div>
      </div>
    </div>

    <!-- BTW -->
    <div v-if="state.btwOpen" :class="dialogBackdrop" @mousedown.self="state.btwOpen = false">
      <div :class="dialog">
        <div :class="dialogTitle">BTW</div>
        <div :class="dialogBody">
          <div v-if="state.btwPrompt" :class="listItem">
            <div class="body">{{ state.btwPrompt }}</div>
          </div>
          <div v-if="state.btwAnswer" :class="listItem">
            <div class="body">{{ state.btwAnswer }}</div>
          </div>
          <div v-else-if="state.btwBusy" :class="listItemSub">…</div>
          <input :class="searchInput" placeholder="Ask by the way…" :value="state.btwDraft" @keydown.enter="btwSend" @keydown.esc="state.btwOpen = false" @input="state.btwDraft = ($event.target as HTMLInputElement).value" />
          <div :class="listItemSub" style="margin-top: 8px">Enter to ask · Esc to close</div>
        </div>
      </div>
    </div>

    <!-- Approval -->
    <div v-if="state.currentApproval" :class="dialogBackdrop">
      <div :class="dialog">
        <div :class="dialogTitle">{{ state.currentApproval.toolName || 'Approval required' }}</div>
        <div :class="dialogBody">
          <div class="body" style="font-family: ui-monospace, 'SF Mono', Menlo, monospace">{{ state.currentApproval.action }}</div>
          <pre v-if="state.currentApproval.command" class="body" style="font-family: ui-monospace, 'SF Mono', Menlo, monospace; white-space: pre-wrap; word-break: break-word; background: #141414; padding: 6px 8px; border: 1px solid rgba(255, 255, 255, 0.08); border-radius: 8px; max-height: 72px; overflow: auto">{{ state.currentApproval.command }}</pre>
          <div style="margin-top: 8px">
            <div
              v-for="(opt, i) in APPROVAL_CHOICES"
              :key="i"
              :class="[listItem, { [listItemSelected]: i === state.approvalSelectedIndex }]"
              @mousedown.prevent="dispatch(Msg.ApprovalSelect(i)); dispatch(Msg.ApprovalConfirm())"
            >{{ opt.label }}</div>
          </div>
          <input
            v-if="state.approvalSelectedIndex === 3"
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
        <div :class="dialogTitle">Question</div>
        <div :class="dialogBody">
          <div style="display: flex; gap: 4px; flex-wrap: wrap; margin-bottom: 8px">
            <span
              v-for="(qq, i) in (state.currentQuestion.allQuestions ?? [state.currentQuestion])"
              :key="i"
              :class="[badge, i === (state.currentQuestion.questionTabIndex ?? 0) ? badgePrimary : badgeOutline]"
            >Q{{ i + 1 }}{{ hasAnswer(qq) ? ' ✓' : '' }}</span>
            <span :class="[badge, (state.currentQuestion.questionTabIndex ?? 0) === questionTabs(state.currentQuestion) - 1 ? badgePrimary : badgeOutline]">Submit</span>
          </div>
          <div :class="listItemTitle">{{ state.currentQuestion.question }}</div>
          <div style="margin-top: 8px">
            <div
              v-for="(opt, i) in (state.currentQuestion.options ?? [])"
              :key="i"
              :class="[listItem, { [listItemSelected]: i === state.questionSelectedIndex }]"
              @mousedown.prevent="state.currentQuestion.kind === 'multi' ? dispatch({ type: 'question_toggle', index: i }) : (dispatch({ type: 'question_select', index: i }), dispatch(Msg.QuestionConfirm()))"
            >{{ (state.currentQuestion.kind === 'multi' || state.currentQuestion.kind === 'multi_with_other') ? (opt.selected ? '✓ ' : '○ ') : (i === state.questionSelectedIndex ? '● ' : '○ ') }}{{ opt.label }}</div>
          </div>
          <input
            v-if="state.currentQuestion.allowOther"
            :class="searchInput"
            :placeholder="state.currentQuestion.otherLabel || 'Other…'"
            :value="state.currentQuestion.otherText ?? ''"
            @input="dispatch({ type: 'question_other', text: ($event.target as HTMLInputElement).value })"
          />
          <div :class="listItemSub" style="margin-top: 8px">←/→ tabs · 1-9 select · space toggle · Enter confirm</div>
        </div>
        <div :class="dialogFooter">
          <button :class="[btn, btnPrimary]" @click="dispatch(Msg.QuestionConfirm())">Submit</button>
        </div>
      </div>
    </div>
  </div>
</template>
