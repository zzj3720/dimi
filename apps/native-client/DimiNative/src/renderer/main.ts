// Dimi native client — Vue entry (vite-plus + Vue 3 + TS).
import { createApp } from 'vue';
import App from './App.vue';
import './styles/global';
import { boot } from './api';

createApp(App).mount('#app');

void boot();
