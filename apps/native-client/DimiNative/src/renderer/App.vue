<script setup lang="ts">
// Root shell: Codex-style sidebar + header + transcript + composer + dialogs.
import { onMounted, onUnmounted } from 'vue';
import Sidebar from './components/Sidebar.vue';
import HeaderBar from './components/HeaderBar.vue';
import Transcript from './components/Transcript.vue';
import Composer from './components/Composer.vue';
import Dialogs from './components/Dialogs.vue';
import { state, Msg } from './store';
import { dispatch } from './api';

// Global keyboard: 1s tick for exit-confirm/undo windows.
const tick = setInterval(() => dispatch(Msg.Tick()), 1000);

onUnmounted(() => clearInterval(tick));
</script>

<template>
  <div class="app">
    <div class="shell">
      <Sidebar />
      <div class="main-col">
        <HeaderBar />
        <Transcript />
        <Composer />
      </div>
    </div>
    <Dialogs />
  </div>
</template>
