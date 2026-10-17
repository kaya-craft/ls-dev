<script setup lang="ts">
import { computed, nextTick, ref, watch } from 'vue'
import { clearLogs, setPaused, useLogs } from './store/logs'
import { BTN_ROW } from '../../ui/layout'

const logs = useLogs()
const box = ref<HTMLElement | null>(null)
const visible = computed(() => {
  const needle = logs.filter.trim().toLowerCase()
  return needle ? logs.lines.filter((l) => l.line.toLowerCase().includes(needle)) : logs.lines
})

watch(
  () => logs.version,
  async () => {
    if (logs.paused) return
    await nextTick()
    box.value?.scrollTo({ top: box.value.scrollHeight })
  },
)
</script>

<template>
  <div class="flex min-h-0 flex-1 flex-col">
    <p v-if="logs.unavailable" class="m-4 rounded-[8px] border border-line bg-soft p-3 text-[12.5px] text-ink-2">
      Started outside LsDev, no logs available. Restart it from LsDev to capture its output.
    </p>
    <template v-else>
      <div class="flex items-center gap-2 border-b border-line px-3 py-2">
        <input v-model="logs.filter" type="search" placeholder="Filter lines" class="h-6 min-w-0 flex-1 rounded-[8px] border border-line-2 bg-transparent px-2 text-xs outline-none focus:border-accent" />
        <button :class="BTN_ROW" @click="setPaused(!logs.paused)">{{ logs.paused ? `Resume (${logs.pending.length})` : 'Pause' }}</button>
        <button :class="BTN_ROW" @click="clearLogs()">Clear</button>
      </div>
      <p v-if="logs.error" class="border-b border-danger bg-soft px-3 py-2 text-[12.5px] text-danger">{{ logs.error }}</p>
      <div ref="box" class="min-h-0 flex-1 overflow-auto bg-term-bg px-3 py-2.5 font-mono text-[11.5px] leading-5 text-term-fg">
        <div
          v-for="(l, i) in visible"
          :key="i"
          class="whitespace-pre-wrap break-all"
          :class="l.system ? 'text-term-dim italic' : l.stream === 'stderr' ? 'text-term-err' : ''"
        >{{ l.line }}</div>
      </div>
    </template>
  </div>
</template>
