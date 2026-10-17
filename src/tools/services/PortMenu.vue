<script setup lang="ts">
import { ref } from 'vue'
import type { Port } from './types'
import { openBrowser } from './actions'
import { BTN, BTN_ROW } from '../../ui/layout'

const props = defineProps<{ ports: Port[]; compact?: boolean }>()
const open = ref(false)

function click() {
  if (props.ports.length === 1) void openBrowser(props.ports[0].port)
  else open.value = !open.value
}
</script>

<template>
  <span v-if="ports.length > 0" class="relative">
    <button :class="compact ? BTN_ROW : BTN" title="Open in browser" @click="click">Open ↗</button>
    <span
      v-if="open"
      class="absolute right-0 top-full z-20 mt-1 flex min-w-[140px] flex-col rounded-[10px] border border-line-2 bg-card p-1 shadow-card"
      @mouseleave="open = false"
    >
      <button
        v-for="p in ports"
        :key="p.port"
        class="rounded-[6px] px-2.5 py-1 text-left font-mono text-xs hover:bg-hover"
        @click="openBrowser(p.port); open = false"
      >localhost:{{ p.port }}</button>
    </span>
  </span>
</template>
