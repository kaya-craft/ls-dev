<script setup lang="ts">
import { computed, onUnmounted, ref, watch } from 'vue'
import { isProcess } from './types'
import { select, useServices } from './store/services'
import { closeLogs, openLogs } from './store/logs'
import { killService, restartService, runContainerAction } from './actions'
import { BTN_GHOST, BTN_ROW, BTN_ROW_DANGER } from '../../ui/layout'
import LogView from './LogView.vue'
import { isDrawer } from '../../shell/viewport'
import { openShare } from '../tunnels/store'

const { selected } = useServices()
const tab = ref<'details' | 'logs'>('details')
const proc = computed(() => (selected.value && isProcess(selected.value) ? selected.value : null))
const ctr = computed(() => (selected.value && !isProcess(selected.value) ? selected.value : null))

// A process with a port is listening; a container needs to be running to publish its ports.
const sharePorts = computed(() => {
  if (proc.value) return proc.value.ports
  return ctr.value && ctr.value.state === 'running' ? ctr.value.ports : []
})
function share(port: number) {
  if (proc.value) openShare({ port, project: proc.value.project, cwd: proc.value.cwd })
  else if (ctr.value) openShare({ port, project: ctr.value.name, cwd: null })
}

watch(
  () => selected.value?.id,
  (id, old) => {
    if (id === old) return
    tab.value = 'details'
    closeLogs()
  },
)
onUnmounted(closeLogs)
watch(tab, (value) => {
  if (value === 'logs' && selected.value) void openLogs(selected.value)
  else closeLogs()
})
</script>

<template>
  <aside
    v-if="selected"
    data-testid="detail-pane"
    :class="[
      'flex w-[440px] max-w-full flex-col overflow-hidden bg-card shadow-card',
      isDrawer
        ? 'absolute inset-y-0 right-0 z-30 border-l border-line'
        : 'm-3 ml-0 shrink-0 rounded-tile border border-line',
    ]"
  >
    <header class="flex items-start justify-between gap-2 px-4 pt-4">
      <div class="min-w-0">
        <h2 class="truncate text-[15px] font-semibold tracking-tight">{{ proc?.project ?? ctr?.name }}</h2>
        <p class="mt-0.5 text-xs text-ink-2">
          <span v-for="p in selected.ports" :key="p.port" class="mr-2 font-mono">:{{ p.port }}</span>
          <span v-if="ctr">{{ ctr.state }}</span>
          <span v-if="proc?.managed" class="ml-1 rounded-full bg-accent-soft px-1.5 py-px text-[11px] font-medium text-accent">managed</span>
        </p>
      </div>
      <button :class="BTN_GHOST" title="Close (Esc)" @click="select(null)">✕</button>
    </header>

    <div class="flex flex-wrap gap-1.5 px-4 py-3" data-testid="detail-actions">
      <template v-if="proc">
        <button :class="BTN_ROW_DANGER" @click="killService(proc)">Kill</button>
        <button :class="BTN_ROW" :disabled="!proc.restart" :title="proc.restart ? '' : 'Working directory or command unknown'" @click="restartService(proc)">Restart</button>
        <button v-for="p in sharePorts" :key="p.port" :class="BTN_ROW" @click="share(p.port)">{{ sharePorts.length > 1 ? `Share :${p.port}` : 'Share publicly…' }}</button>
        <slot name="process-extra" :service="proc" />
      </template>
      <template v-if="ctr">
        <button v-if="ctr.state !== 'running'" :class="BTN_ROW" @click="runContainerAction(ctr, 'start')">Start</button>
        <button v-else :class="BTN_ROW" @click="runContainerAction(ctr, 'stop')">Stop</button>
        <button :class="BTN_ROW" @click="runContainerAction(ctr, 'restart')">Restart</button>
        <button :class="BTN_ROW_DANGER" @click="runContainerAction(ctr, 'remove')">Remove</button>
        <button v-for="p in sharePorts" :key="p.port" :class="BTN_ROW" @click="share(p.port)">{{ sharePorts.length > 1 ? `Share :${p.port}` : 'Share publicly…' }}</button>
        <slot name="container-extra" :service="ctr" />
      </template>
    </div>

    <nav class="flex gap-4 border-b border-line px-4 text-[12.5px]">
      <button
        v-for="t in ['details', 'logs'] as const"
        :key="t"
        class="-mb-px border-b-2 pb-2 capitalize"
        :class="tab === t ? 'border-accent font-medium text-ink' : 'border-transparent text-ink-2 hover:text-ink'"
        @click="tab = t"
      >{{ t }}</button>
    </nav>

    <div v-if="tab === 'details'" class="min-h-0 flex-1 overflow-auto px-4 py-4 text-[12.5px]">
      <dl v-if="proc" class="grid grid-cols-[90px_minmax(0,1fr)] gap-x-3 gap-y-2.5">
        <dt class="text-ink-3">Directory</dt><dd class="break-all font-mono text-xs">{{ proc.cwd ?? '—' }}</dd>
        <dt class="text-ink-3">Command</dt><dd class="break-all font-mono text-xs">{{ proc.command || '—' }}</dd>
        <dt v-if="proc.worktree" class="text-ink-3">Worktree</dt>
        <dd v-if="proc.worktree" class="break-all font-mono text-xs">{{ proc.worktree.branch }} · {{ proc.worktree.path }}</dd>
        <dt class="text-ink-3">Process tree</dt>
        <dd class="min-w-0 rounded-[8px] bg-soft p-2">
          <div v-for="n in proc.tree" :key="n.pid" class="truncate font-mono text-xs" :style="{ paddingLeft: `${n.depth * 12}px` }" :title="n.command">
            {{ n.pid }} {{ n.command }}
          </div>
        </dd>
      </dl>
      <dl v-if="ctr" class="grid grid-cols-[90px_minmax(0,1fr)] gap-x-3 gap-y-2.5">
        <dt class="text-ink-3">Image</dt><dd class="break-all font-mono text-xs">{{ ctr.image }}</dd>
        <dt class="text-ink-3">Status</dt><dd>{{ ctr.status }}</dd>
        <dt class="text-ink-3">ID</dt><dd class="font-mono text-xs">{{ ctr.containerId.slice(0, 12) }}</dd>
        <template v-if="ctr.compose">
          <dt class="text-ink-3">Project</dt><dd>{{ ctr.compose.project }}</dd>
          <dt class="text-ink-3">Service</dt><dd>{{ ctr.compose.service }}</dd>
          <dt class="text-ink-3">Directory</dt><dd class="break-all font-mono text-xs">{{ ctr.compose.workingDir ?? '—' }}</dd>
        </template>
      </dl>
    </div>
    <LogView v-else />
  </aside>
</template>
