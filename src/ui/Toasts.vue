<script setup lang="ts">
import { dismiss, useToasts } from './store/toasts'
const state = useToasts()
</script>

<template>
  <div class="fixed bottom-4 right-4 z-50 flex w-96 flex-col gap-2">
    <div
      v-for="t in state.items"
      :key="t.id"
      role="status"
      class="cursor-pointer whitespace-pre-line rounded-[10px] px-3.5 py-2.5 text-[12.5px] shadow-card"
      :class="t.kind === 'error' ? 'bg-danger text-accent-fg' : 'bg-ink text-bg'"
      @click="dismiss(t.id)"
    >{{ t.message }}</div>
  </div>
</template>
