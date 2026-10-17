<script setup lang="ts">
import { railShown } from '../shell/viewport'

defineProps<{ title: string; subtitle: string }>()
// The macOS traffic lights overlay the rail top-left when the sidebar is shown as a rail.
defineEmits<{ 'open-settings': [] }>()
</script>

<template>
  <header data-tauri-drag-region :class="['flex h-[60px] shrink-0 items-center gap-2.5 pr-3', railShown ? 'pl-[76px]' : 'pl-5']">
    <div data-tauri-drag-region class="min-w-0">
      <h1 class="text-base font-semibold tracking-tight">{{ title }}</h1>
      <div class="text-[11.5px] text-ink-3">{{ subtitle }}</div>
    </div>
    <div data-tauri-drag-region class="flex-1 self-stretch" />
    <slot name="action" />
    <button
      type="button"
      aria-label="Settings"
      title="Settings (⌘,)"
      class="flex h-[30px] w-[30px] items-center justify-center rounded-nav text-ink-2 hover:bg-hover"
      @click="$emit('open-settings')"
    >
      <svg class="h-[15px] w-[15px]" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round">
        <path d="M4 6h9M17 6h3M4 12h3M11 12h9M4 18h11M19 18h1" /><circle cx="15" cy="6" r="2" /><circle cx="9" cy="12" r="2" /><circle cx="17" cy="18" r="2" />
      </svg>
    </button>
  </header>
</template>
