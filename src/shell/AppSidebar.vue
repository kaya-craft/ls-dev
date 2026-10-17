<script setup lang="ts">
import { computed } from 'vue'
import { useServices } from '../tools/services/store/services'
import { useWorktrees } from '../tools/worktrees/store'
import { TOOLS, setServicesSection, setTool, toggleSidebar, useNavigation, type ServicesSection, type Tool } from './navigation'
import { isNarrow, railShown } from './viewport'

const { state } = useNavigation()
const services = useServices()
const worktrees = useWorktrees()

const collapsed = railShown
const showSections = computed(() => state.currentTool === 'services' && !collapsed.value)
const dockerUp = computed(() => services.state.snapshot?.dockerAvailable ?? false)

const ICONS: Record<Tool, string[]> = {
  services: [
    'circle:12:12:2.5',
    'M7.8 7.8a6 6 0 0 0 0 8.4M16.2 7.8a6 6 0 0 1 0 8.4M5 5a10 10 0 0 0 0 14M19 5a10 10 0 0 1 0 14',
  ],
  worktrees: [
    'circle:6:5:2',
    'circle:6:19:2',
    'circle:18:7:2',
    'M6 7v10M18 9c0 4-3 6-12 8',
  ],
  databases: [
    'ellipse:12:5.5:7.5:2.5',
    'M4.5 5.5v13c0 1.4 3.4 2.5 7.5 2.5s7.5-1.1 7.5-2.5v-13',
    'M4.5 12c0 1.4 3.4 2.5 7.5 2.5s7.5-1.1 7.5-2.5',
  ],
  tunnels: [
    'M10 14a4 4 0 0 0 5.7 0l3-3a4 4 0 0 0-5.7-5.7l-1 1',
    'M14 10a4 4 0 0 0-5.7 0l-3 3a4 4 0 0 0 5.7 5.7l1-1',
  ],
}

function shapes(tool: Tool) {
  return ICONS[tool].map((d) => {
    const [kind, a, b, c, e] = d.split(':')
    if (kind === 'circle') return { kind, cx: a, cy: b, r: c }
    if (kind === 'ellipse') return { kind, cx: a, cy: b, rx: c, ry: e }
    return { kind: 'path', d }
  })
}

const compose = computed(() => services.state.snapshot?.compose ?? [])
const standaloneCount = computed(() => {
  const list = services.state.snapshot?.standalone ?? []
  return `${list.filter((c) => c.state === 'running').length}/${list.length}`
})
const devCount = computed(() => services.state.snapshot?.processes.length ?? 0)

function sameSection(a: ServicesSection, b: ServicesSection) {
  if (typeof a === 'string' || typeof b === 'string') return a === b
  return a.compose === b.compose
}
function isCurrent(section: ServicesSection) {
  return sameSection(services.effectiveSection.value, section)
}
const SUB_BASE = 'flex w-full items-center rounded-nav px-2.5 py-1 text-left text-xs'
function subClass(section: ServicesSection) {
  return [SUB_BASE, isCurrent(section) ? 'bg-active text-ink' : 'text-ink-2 hover:bg-hover']
}
function toolClass(tool: Tool) {
  return [
    'relative flex items-center rounded-nav text-left mt-1',
    collapsed.value ? 'h-9 w-9 justify-center' : 'w-full gap-2.5 px-2.5 py-1.5',
    state.currentTool === tool ? 'bg-active text-ink' : 'text-ink-2 hover:bg-hover',
  ]
}
</script>

<template>
  <nav
    aria-label="Tools"
    :class="[
      'flex shrink-0 flex-col border-r border-line bg-side pb-3',
      collapsed ? 'w-[60px] items-center gap-1 px-0' : 'w-[220px] px-2.5',
    ]"
  >
    <!-- Top 44 px: the macOS traffic lights overlay this strip (window titleBarStyle Overlay). -->
    <div data-tauri-drag-region class="flex h-11 w-full shrink-0 items-center justify-end">
      <button
        v-if="!collapsed"
        type="button"
        aria-label="Collapse sidebar"
        class="flex h-[26px] w-[26px] items-center justify-center rounded-nav text-ink-2 hover:bg-hover"
        @click="toggleSidebar"
      >
        <svg class="h-[15px] w-[15px]" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round">
          <rect x="3" y="4" width="18" height="16" rx="2.5" /><path d="M9 4v16" />
        </svg>
      </button>
    </div>
    <div v-if="!collapsed" class="px-2.5 pb-3.5 pt-1 text-base font-bold tracking-tight">LsDev<span class="text-warm">.</span></div>
    <div v-else aria-hidden="true" class="mb-3.5 flex h-8 w-8 items-center justify-center rounded-nav bg-card text-sm font-bold">
      L<span class="text-warm">.</span>
    </div>

    <template v-for="tool in TOOLS" :key="tool.id">
      <button
        type="button"
        data-testid="tool"
        :aria-label="tool.label"
        :title="collapsed ? `${tool.label} ${tool.shortcut}` : undefined"
        :class="toolClass(tool.id)"
        @click="setTool(tool.id)"
      >
        <svg class="h-[15px] w-[15px] shrink-0" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round">
          <template v-for="(s, i) in shapes(tool.id)" :key="i">
            <circle v-if="s.kind === 'circle'" :cx="s.cx" :cy="s.cy" :r="s.r" />
            <ellipse v-else-if="s.kind === 'ellipse'" :cx="s.cx" :cy="s.cy" :rx="s.rx" :ry="s.ry" />
            <path v-else :d="s.d" />
          </template>
        </svg>
        <template v-if="!collapsed">
          <span class="flex-1">{{ tool.label }}</span>
          <span v-if="tool.id === 'services'" class="font-mono text-xs text-ink-3">{{ services.listeningCount.value }}</span>
          <span v-else-if="tool.id === 'worktrees' && worktrees.state.snapshot" class="font-mono text-xs text-ink-3">{{ worktrees.summary.value.count }}</span>
          <span v-else class="font-mono text-xs text-ink-3">{{ tool.shortcut }}</span>
        </template>
        <span
          v-else-if="tool.id === 'services' && services.listeningCount.value > 0"
          class="absolute -right-0.5 -top-0.5 min-w-4 rounded-full bg-accent px-1 text-center font-mono text-[10px] leading-4 text-accent-fg"
        >{{ services.listeningCount.value }}</span>
      </button>

      <div v-if="tool.id === 'services' && showSections" class="mb-1 mt-1 ml-4 flex flex-col gap-0.5 border-l border-line pl-2">
        <button type="button" :class="subClass('all')" @click="setServicesSection('all')">
          <span class="flex-1">All listening</span>
          <span class="font-mono text-ink-3">{{ services.listeningCount.value }}</span>
        </button>
        <button type="button" :class="subClass('dev')" @click="setServicesSection('dev')">
          <span class="flex-1">Dev servers</span>
          <span class="font-mono text-ink-3">{{ devCount }}</span>
        </button>
        <button
          v-for="g in compose"
          :key="g.project"
          type="button"
          :class="subClass({ compose: g.project })"
          @click="setServicesSection({ compose: g.project })"
        >
          <span class="flex-1 truncate">{{ g.project }}</span>
          <span class="font-mono text-ink-3">{{ g.containers.filter((c) => c.state === 'running').length }}/{{ g.containers.length }}</span>
        </button>
        <button type="button" :class="subClass('standalone')" @click="setServicesSection('standalone')">
          <span class="flex-1">Standalone</span>
          <span class="font-mono text-ink-3">{{ standaloneCount }}</span>
        </button>
      </div>
    </template>

    <div class="flex-1" />

    <div
      v-if="!collapsed"
      class="flex items-center gap-2 border-t border-line px-2.5 pt-2.5 text-[11.5px] text-ink-3"
    >
      <span :class="['h-[7px] w-[7px] rounded-full', dockerUp ? 'bg-ok' : 'bg-idle']" />
      {{ dockerUp ? 'Docker running' : 'Docker not reachable' }}
    </div>
    <template v-else>
      <span
        :title="dockerUp ? 'Docker running' : 'Docker not reachable'"
        :class="['mb-2 h-[7px] w-[7px] rounded-full', dockerUp ? 'bg-ok' : 'bg-idle']"
      />
      <button
        v-if="!isNarrow"
        type="button"
        aria-label="Expand sidebar"
        class="flex h-8 w-8 items-center justify-center rounded-nav text-ink-2 hover:bg-hover"
        @click="toggleSidebar"
      >
        <svg class="h-[15px] w-[15px]" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round">
          <rect x="3" y="4" width="18" height="16" rx="2.5" /><path d="M9 4v16" />
        </svg>
      </button>
    </template>
  </nav>
</template>
