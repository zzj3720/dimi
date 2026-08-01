<script setup lang="ts">
import logoUrl from '../Dimi.png'

withDefaults(defineProps<{ size?: number }>(), { size: 56 })
</script>

<template>
  <img
    class="DimiLogo"
    :src="logoUrl"
    :width="size"
    :height="size"
    alt="Dimi"
  />
</template>

<style scoped>
.DimiLogo {
  display: block;
  object-fit: contain;
}
</style>
