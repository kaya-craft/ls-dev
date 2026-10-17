<script setup lang="ts">
import { computed } from 'vue'
import Sparkline from './Sparkline.vue'
import { useServices } from './store/services'
import { isNarrow } from '../../shell/viewport'
import { formatBytes, formatCpu } from '../../ui/format'

const { stats, history } = useServices()

const mem = computed(() => {
  const [value, unit] = formatBytes(stats.value.mem).split(' ')
  return { value, unit: unit ?? '' }
})

const tileClass = computed(() =>
  isNarrow.value
    ? 'flex min-w-0 flex-col gap-0.5 rounded-xl border border-line bg-card px-3 py-[9px] shadow-card'
    : 'flex min-w-0 items-end justify-between gap-2 rounded-tile border border-line bg-card px-3.5 py-[11px] shadow-card',
)
const kClass = computed(() => ['k', isNarrow.value ? 'text-[11px]' : 'text-[11.5px]', 'text-ink-3'])
const vClass = computed(() => [
  'v whitespace-nowrap font-mono font-semibold tracking-tight tabular-nums',
  isNarrow.value ? 'text-[18px]' : 'text-[21px] leading-[1.15]',
])
const smallClass = computed(() => [
  'font-normal tracking-normal text-ink-3',
  isNarrow.value ? 'ml-[3px] text-[11px]' : 'ml-1 text-[11.5px]',
])
const col = 'flex min-w-0 flex-col gap-[3px]'
</script>

<template>
  <div :class="['grid shrink-0 grid-cols-4', isNarrow ? 'gap-2.5 px-3.5 pb-3' : 'gap-3 px-4 pb-3.5']">
    <div data-testid="tile" :class="tileClass">
      <div :class="isNarrow ? 'contents' : col">
        <span :class="kClass">{{ stats.isCompose || isNarrow ? 'Ports' : 'Listening ports' }}</span>
        <span :class="vClass">
          {{ stats.ports }} <small :class="smallClass">{{ isNarrow ? `${stats.responding} up` : `${stats.responding} responding` }}</small>
        </span>
      </div>
    </div>
    <div data-testid="tile" :class="tileClass">
      <div :class="isNarrow ? 'contents' : col">
        <span :class="kClass">{{ stats.isCompose ? 'Containers' : 'Services' }}</span>
        <span :class="vClass">
          {{ stats.running }} <small :class="smallClass">{{ isNarrow ? `· ${stats.stopped} off` : `running · ${stats.stopped} stopped` }}</small>
        </span>
      </div>
    </div>
    <div data-testid="tile" :class="tileClass">
      <div :class="isNarrow ? 'contents' : col">
        <span :class="kClass">CPU</span>
        <span :class="[vClass, isNarrow ? 'text-accent' : '']">{{ formatCpu(stats.cpu) }}</span>
      </div>
      <Sparkline v-if="!isNarrow" class="text-accent" :series="history.cpu" />
    </div>
    <div data-testid="tile" :class="tileClass">
      <div :class="isNarrow ? 'contents' : col">
        <span :class="kClass">Memory</span>
        <span :class="[vClass, isNarrow ? 'text-warm' : '']">{{ mem.value }} <small :class="smallClass">{{ mem.unit }}</small></span>
      </div>
      <Sparkline v-if="!isNarrow" class="text-warm" :series="history.mem" />
    </div>
  </div>
</template>
