<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import { COMPACT_BELOW, tableCompact } from './compact'
import ServiceIcon from './ServiceIcon.vue'
import ServiceRow from './ServiceRow.vue'
import { meterScale } from './rowText'
import { select, useServices } from './store/services'
import { isCollapsed, toggleCollapsed, toggleSort, useView } from './store/view'
import type { SortKey } from './sort'
import type { ComposeGroup } from './types'
import { isNarrow } from '../../shell/viewport'
import { formatBytes, formatCpu } from '../../ui/format'
import { ROW_GRID, ROW_GRID_COMPACT, ROW_GRID_NARROW } from '../../ui/layout'

const { state, sections, loading, effectiveSection } = useServices()
// Outside the global view, only the block of the chosen section is rendered.
const showDev = computed(() => ['all', 'dev'].includes(effectiveSection.value as string))
const showCompose = computed(() => effectiveSection.value === 'all' || typeof effectiveSection.value !== 'string')
const showStandalone = computed(() => ['all', 'standalone'].includes(effectiveSection.value as string))
const root = ref<HTMLElement | null>(null)
let observer: ResizeObserver | null = null
// Below COMPACT_BELOW the table drops uptime and meters, whatever the window width.
const narrow = computed(() => isNarrow.value || tableCompact.value)
const grid = computed(() =>
  tableCompact.value ? ROW_GRID_COMPACT : isNarrow.value ? ROW_GRID_NARROW : ROW_GRID,
)
const view = useView().state
const sortHeads = computed<Array<{ key: SortKey; label: string; align: string }>>(() => [
  ...(narrow.value ? [] : [{ key: 'uptime' as const, label: 'Uptime', align: 'justify-end' }]),
  { key: 'cpu', label: 'CPU', align: 'justify-end' },
  { key: 'memory', label: 'Memory', align: 'justify-end' },
])
function ariaSort(key: SortKey) {
  if (view.sort?.key !== key) return 'none'
  return view.sort.dir === 'desc' ? 'descending' : 'ascending'
}
function arrow(key: SortKey) {
  if (view.sort?.key !== key) return ''
  return view.sort.dir === 'desc' ? '↓' : '↑'
}
const sectionClass =
  'flex h-[38px] items-center border-b border-line bg-soft px-4 text-xs font-semibold text-ink-2'

// A non-empty filter expands every group without touching the stored list.
const filtering = computed(() => state.filter.trim() !== '')
function expanded(project: string) {
  return filtering.value || !isCollapsed(project)
}
function sumOrNull(values: Array<number | null>): number | null {
  const present = values.filter((v): v is number => v !== null)
  return present.length === 0 ? null : present.reduce((a, b) => a + b, 0)
}
function summary(g: ComposeGroup) {
  const running = g.containers.filter((c) => c.state === 'running').length
  const cpu = formatCpu(sumOrNull(g.containers.map((c) => c.cpuPercent)))
  const mem = formatBytes(sumOrNull(g.containers.map((c) => c.memBytes)))
  const n = g.containers.length
  if (narrow.value) return `${running}/${n} · ${cpu} · ${mem}`
  return `${n} ${n === 1 ? 'container' : 'containers'} · ${running} running · ${cpu} · ${mem}`
}
const standaloneRunning = computed(() => sections.value.standalone.filter((c) => c.state === 'running').length)

// Meter scales come from the rows currently displayed (collapsed groups excluded).
const displayed = computed(() => [
  ...sections.value.processes,
  ...sections.value.compose.flatMap((g) => (expanded(g.project) ? g.containers : [])),
  ...sections.value.standalone,
])
const cpuScale = computed(() => meterScale('cpu', displayed.value.map((s) => s.cpuPercent)))
const memScale = computed(() => meterScale('mem', displayed.value.map((s) => s.memBytes)))

// The `…` menu holding the group actions below 960 px; one open at a time.
const menuOpen = ref<string | null>(null)
function toggleMenu(project: string) {
  menuOpen.value = menuOpen.value === project ? null : project
}
function onDocumentClick(event: MouseEvent) {
  if (menuOpen.value && !(event.target as Element | null)?.closest('[data-group-menu]')) menuOpen.value = null
}
function onFocusOut(event: FocusEvent) {
  const host = event.currentTarget as HTMLElement
  if (!host.contains(event.relatedTarget as Node | null)) menuOpen.value = null
}
// Capture phase: closing the menu must stop the global Esc handler (it would clear the selection).
function onDocumentKeydown(event: KeyboardEvent) {
  if (event.key !== 'Escape' || menuOpen.value === null) return
  menuOpen.value = null
  event.stopPropagation()
}
onMounted(() => {
  if (typeof ResizeObserver !== 'undefined' && root.value) {
    observer = new ResizeObserver((entries) => {
      const width = entries[0]?.contentRect.width ?? root.value?.clientWidth ?? 0
      tableCompact.value = width > 0 && width < COMPACT_BELOW
    })
    observer.observe(root.value)
  }
  document.addEventListener('click', onDocumentClick)
  document.addEventListener('keydown', onDocumentKeydown, true)
})
onBeforeUnmount(() => {
  observer?.disconnect()
  tableCompact.value = false
  document.removeEventListener('click', onDocumentClick)
  document.removeEventListener('keydown', onDocumentKeydown, true)
})
const emptyClass = 'px-4 py-3 text-[12.5px] text-ink-3'
</script>

<template>
  <div ref="root" role="table" class="text-sm">
    <div
      role="row"
      :class="[
        grid,
        'sticky top-0 z-10 h-8 border-b border-line bg-card text-[11.5px] font-medium text-ink-3',
      ]"
    >
      <div role="columnheader">Ports</div>
      <div role="columnheader">Service</div>
      <div
        v-for="h in sortHeads"
        :key="h.key"
        role="columnheader"
        :aria-sort="ariaSort(h.key)"
        class="flex"
        :class="h.align"
      >
        <button
          type="button"
          :data-sort="h.key"
          class="inline-flex items-center gap-0.5 hover:text-ink"
          :class="view.sort?.key === h.key ? 'text-ink' : ''"
          @click="toggleSort(h.key)"
        >{{ h.label }}<span aria-hidden="true" class="w-3">{{ arrow(h.key) }}</span></button>
      </div>
    </div>

    <p v-if="loading" class="px-4 py-6 text-[12.5px] text-ink-3">Collecting…</p>

    <template v-else>
      <section v-if="showDev" aria-label="Dev servers">
        <h2 :class="sectionClass">Dev servers · {{ sections.processes.length }}</h2>
        <ServiceRow
          v-for="p in sections.processes"
          :key="p.id"
          :service="p"
          :selected="state.selectedId === p.id"
          :cpu-scale="cpuScale"
          :mem-scale="memScale"
          @select="select"
        >
          <template #actions><slot name="process-actions" :service="p" /></template>
        </ServiceRow>
        <p v-if="sections.processes.length === 0" :class="emptyClass">Nothing listening</p>
      </section>

      <p
        v-if="(showCompose || showStandalone) && state.snapshot && !state.snapshot.dockerAvailable"
        role="alert"
        data-testid="docker-banner"
        class="m-4 flex items-center gap-2.5 rounded-[10px] border border-line-2 bg-soft px-4 py-3 text-[12.5px] text-ink-2"
      >
        <span aria-hidden="true" class="size-2 shrink-0 rounded-full bg-idle" />
        Docker not reachable. Start Docker Desktop to see containers.
      </p>

      <template v-else-if="showCompose || showStandalone">
        <section v-for="g in (showCompose ? sections.compose : [])" :key="g.project" :aria-label="g.project">
          <h2 :class="[sectionClass, 'gap-2 pl-2']">
            <button
              type="button"
              class="inline-flex h-6 items-center gap-1.5 rounded-[6px] pr-1 hover:text-ink disabled:opacity-60"
              :aria-expanded="expanded(g.project)"
              :disabled="filtering"
              :title="filtering ? 'Groups are expanded while filtering' : undefined"
              @click="toggleCollapsed(g.project)"
            >
              <svg
                viewBox="0 0 24 24"
                aria-hidden="true"
                :class="['size-[15px] flex-none fill-none stroke-current transition-transform', expanded(g.project) ? 'rotate-90' : '']"
                stroke-width="1.7"
                stroke-linecap="round"
                stroke-linejoin="round"
              ><path d="m9 6 6 6-6 6" /></svg>
              {{ g.project }}
            </button>
            <span class="ml-0.5 inline-flex" aria-hidden="true">
              <ServiceIcon
                v-for="(c, i) in g.containers"
                :key="c.id"
                :service="c"
                mini
                data-testid="mini-pastille"
                :class="i > 0 ? '-ml-[5px]' : ''"
              />
            </span>
            <span
              v-if="!expanded(g.project)"
              data-testid="group-summary"
              class="min-w-0 truncate font-mono text-[11.5px] font-normal tabular-nums text-ink-3"
            >{{ summary(g) }}</span>
            <span class="flex-1" />
            <span v-if="!narrow" class="flex gap-1"><slot name="group-actions" :group="g" /></span>
            <span v-else class="relative" data-group-menu @focusout="onFocusOut" @mouseleave="menuOpen = null">
              <button
                type="button"
                aria-label="Group actions"
                :aria-expanded="menuOpen === g.project"
                class="inline-flex h-6 w-7 items-center justify-center rounded-[6px] text-ink-2 hover:bg-hover"
                @click="toggleMenu(g.project)"
              >…</button>
              <span
                v-if="menuOpen === g.project"
                class="absolute right-0 top-full z-20 mt-1 flex flex-col items-stretch rounded-[8px] border border-line bg-card p-1 shadow-card"
                @click="menuOpen = null"
              ><slot name="group-actions" :group="g" /></span>
            </span>
          </h2>
          <ServiceRow
            v-for="c in expanded(g.project) ? g.containers : []"
            :key="c.id"
            :service="c"
            :selected="state.selectedId === c.id"
            :cpu-scale="cpuScale"
            :mem-scale="memScale"
            @select="select"
          >
            <template #actions><slot name="container-actions" :service="c" /></template>
          </ServiceRow>
        </section>

        <section v-if="showStandalone" aria-label="Standalone containers">
          <h2 :class="sectionClass">Standalone containers · {{ standaloneRunning }} running</h2>
          <ServiceRow
            v-for="c in sections.standalone"
            :key="c.id"
            :service="c"
            :selected="state.selectedId === c.id"
            :cpu-scale="cpuScale"
            :mem-scale="memScale"
            @select="select"
          >
            <template #actions><slot name="container-actions" :service="c" /></template>
          </ServiceRow>
          <p v-if="sections.standalone.length === 0" :class="emptyClass">Nothing running</p>
        </section>
      </template>
    </template>
  </div>
</template>
