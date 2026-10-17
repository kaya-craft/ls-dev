<script setup lang="ts">
import { computed } from 'vue'
import Meter from './Meter.vue'
import ServiceIcon from './ServiceIcon.vue'
import { openBrowser } from './actions'
import { meterFraction, splitName, subline } from './rowText'
import type { ContainerService, ProcessService } from './types'
import { isProcess } from './types'
import { isNarrow } from '../../shell/viewport'
import { tableCompact } from './compact'
import { formatBytes, formatCpu, formatUptime } from '../../ui/format'
import { forPort, movedNotice } from '../tunnels/store'
import type { Tunnel } from '../tunnels/types'
import { toasts } from '../../ui/store/toasts'
import { ROW_GRID, ROW_GRID_COMPACT, ROW_GRID_NARROW } from '../../ui/layout'

const props = defineProps<{
  service: ProcessService | ContainerService
  selected: boolean
  cpuScale: number
  memScale: number
}>()
const emit = defineEmits<{ select: [id: string] }>()

const proc = computed(() => (isProcess(props.service) ? props.service : null))
const ctr = computed(() => (isProcess(props.service) ? null : props.service))
const stopped = computed(() => ctr.value !== null && ctr.value.state !== 'running')
const name = computed(() => splitName(proc.value?.project ?? ctr.value?.name ?? ''))
const narrow = computed(() => isNarrow.value || tableCompact.value)
const grid = computed(() =>
  tableCompact.value ? ROW_GRID_COMPACT : isNarrow.value ? ROW_GRID_NARROW : ROW_GRID,
)
const line = computed(() => subline(props.service, { narrow: narrow.value }))
const cpu = computed(() => (stopped.value ? null : props.service.cpuPercent))
const mem = computed(() => (stopped.value ? null : props.service.memBytes))

// Two ports fit in 128 px; with three or more (or two when compact, 96 px) show the first and `+N`.
const maxPorts = computed(() => {
  const n = props.service.ports.length
  if (tableCompact.value) return n >= 2 ? 1 : n
  return n <= 2 ? n : 1
})
const shownPorts = computed(() => props.service.ports.slice(0, maxPorts.value))
const hiddenPorts = computed(() => props.service.ports.length - shownPorts.value.length)
const allPortsTitle = computed(() => props.service.ports.map((p) => `:${p.port}`).join(', '))

// Open tunnel of any port of the row, including the ports hidden behind `+N`.
const publicTunnel = computed<Tunnel | null>(() => {
  for (const p of props.service.ports) {
    const t = forPort(p.port)
    if (t?.status === 'open' && t.url) return t
  }
  return null
})
const publicUrl = computed(() => publicTunnel.value?.url ?? null)
// The tunnel was opened for another folder than the process now on its port.
const publicTitle = computed(() => {
  const t = publicTunnel.value
  if (!t) return ''
  return (proc.value && movedNotice(t, [proc.value])) || `Copy ${t.url}`
})
async function copyPublicUrl() {
  if (!publicUrl.value) return
  try {
    await navigator.clipboard.writeText(publicUrl.value)
    toasts.success('Copied')
  } catch (e) {
    toasts.error(e instanceof Error ? e.message : String(e))
  }
}

function dotClass(responsive: boolean | null) {
  if (responsive === true) return 'bg-ok'
  if (responsive === false) return 'bg-idle'
  return 'border border-idle'
}
function dotLabel(responsive: boolean | null) {
  if (responsive === true) return 'responding'
  if (responsive === false) return 'not responding'
  return 'not checked'
}
</script>

<template>
  <div
    role="row"
    :class="[
      grid,
      'group relative h-[50px] cursor-default border-b border-line text-sm',
      selected ? 'bg-accent-soft' : 'hover:bg-row-hover',
      stopped ? 'text-ink-3' : '',
    ]"
    @click="emit('select', service.id)"
  >
    <div data-testid="ports" class="flex flex-nowrap items-center gap-x-3 overflow-hidden whitespace-nowrap font-mono text-[12.5px] tabular-nums">
      <template v-for="p in shownPorts" :key="p.port">
        <button
          v-if="p.responsive === true"
          type="button"
          class="inline-flex items-center gap-1.5 font-medium text-accent hover:underline"
          :title="`Open :${p.port} in browser`"
          :aria-label="`Open :${p.port} in browser`"
          @click.stop="openBrowser(p.port)"
        >
          <span class="size-2 flex-none rounded-full" :class="dotClass(p.responsive)" :aria-label="dotLabel(p.responsive)" />:{{ p.port }}
        </button>
        <span v-else class="inline-flex items-center gap-1.5 text-ink-3">
          <span class="size-2 flex-none rounded-full" :class="dotClass(p.responsive)" :aria-label="dotLabel(p.responsive)" />:{{ p.port }}
        </span>
      </template>
      <span
        v-if="hiddenPorts > 0"
        data-testid="ports-more"
        class="flex-none rounded bg-soft px-1.5 text-[11px] text-ink-3"
        :title="allPortsTitle"
      >+{{ hiddenPorts }}</span>
      <span v-if="service.ports.length === 0" class="text-ink-3">—</span>
    </div>

    <div class="flex min-w-0 items-center gap-[11px]">
      <ServiceIcon :service="service" />
      <div class="min-w-0">
        <div class="flex items-center gap-1.5">
          <span data-testid="name" class="min-w-[80px] truncate">
            <span v-if="name.prefix" data-testid="name-prefix" class="text-ink-3">{{ name.prefix }}</span>
            <span data-testid="name-last" class="font-medium">{{ name.last }}</span>
          </span>
          <span
            v-if="proc?.worktree"
            data-testid="worktree-badge"
            class="min-w-0 truncate rounded bg-branch-bg px-1.5 font-mono text-[11px] text-branch"
          >{{ proc.worktree.branch }}</span>
          <span
            v-if="proc?.managed"
            data-testid="managed-badge"
            class="min-w-0 truncate rounded bg-accent-soft px-1.5 text-[11px] text-accent"
          >managed</span>
          <button
            v-if="publicUrl"
            type="button"
            data-testid="public-badge"
            class="flex-none rounded bg-accent-soft px-1.5 text-[11px] text-accent hover:underline"
            :title="publicTitle"
            @click.stop="copyPublicUrl"
          >Public</button>
        </div>
        <div data-testid="subline" class="mt-0.5 truncate font-mono text-[11px] text-ink-3" :title="line">{{ line }}</div>
      </div>
    </div>

    <div v-if="!narrow" class="text-right font-mono text-xs tabular-nums text-ink-2">
      {{ stopped ? '—' : formatUptime(service.uptimeSec) }}
    </div>

    <div class="flex items-center justify-end gap-2">
      <Meter v-if="cpu !== null && !tableCompact" kind="cpu" :fraction="meterFraction(cpu, cpuScale)" />
      <span class="w-[38px] text-right font-mono text-xs tabular-nums">{{ formatCpu(cpu) }}</span>
    </div>
    <div class="flex items-center justify-end gap-2">
      <Meter v-if="mem !== null && !tableCompact" kind="mem" :fraction="meterFraction(mem, memScale)" />
      <span class="w-[52px] text-right font-mono text-xs tabular-nums">{{ formatBytes(mem) }}</span>
    </div>

    <div
      :class="[
        'pointer-events-none absolute inset-y-0 right-3 flex items-center justify-end gap-1 pl-3 opacity-0 group-focus-within:opacity-100 group-hover:opacity-100 [&_*]:pointer-events-auto',
        selected
          ? 'bg-card bg-[linear-gradient(var(--color-accent-soft),var(--color-accent-soft))]'
          : 'bg-row-hover',
      ]"
      @click.stop
    >
      <slot name="actions" />
    </div>
  </div>
</template>
