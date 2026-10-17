<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { api } from './api'
import { runComposeAction } from './actions'
import { setServicesSection, type ServicesSection } from '../../shell/navigation'
import { isNarrow, railShown } from '../../shell/viewport'
import { setError, setFilter, useServices } from './store/services'
import { sectionKey } from './stats'
import { updateSettings, useSettings } from '../../ui/store/settings'
import { BTN_PAGE } from '../../ui/layout'

const emit = defineEmits<{ 'open-settings': [] }>()
const { state, effectiveSection } = useServices()
const settings = useSettings()
const filterInput = ref<HTMLInputElement | null>(null)
const menuRoot = ref<HTMLElement | null>(null)
const menuOpen = ref(false)

function refresh() {
  api.refresh().catch((e) => setError(String(e)))
}

function toggleShowAll(event: Event) {
  const checked = (event.target as HTMLInputElement).checked
  updateSettings({ showAll: checked }).catch((e) => {
    ;(event.target as HTMLInputElement).checked = !checked
    setError(String(e))
  })
}

const title = computed(() => {
  const s = effectiveSection.value
  if (s === 'all') return 'All listening'
  if (s === 'dev') return 'Dev servers'
  if (s === 'standalone') return 'Standalone'
  return s.compose
})

const group = computed(() => {
  const s = effectiveSection.value
  if (typeof s === 'string') return null
  return state.snapshot?.compose.find((g) => g.project === s.compose) ?? null
})

const choices = computed<Array<{ key: string; label: string; section: ServicesSection }>>(() => {
  const list: Array<{ label: string; section: ServicesSection }> = [
    { label: 'All listening', section: 'all' },
    { label: 'Dev servers', section: 'dev' },
    ...(state.snapshot?.compose ?? []).map((g) => ({ label: g.project, section: { compose: g.project } })),
    { label: 'Standalone', section: 'standalone' },
  ]
  return list.map((c) => ({ ...c, key: sectionKey(c.section) }))
})

function choose(section: ServicesSection) {
  setServicesSection(section)
  menuOpen.value = false
}

function onFocusOut(event: FocusEvent) {
  const host = event.currentTarget as HTMLElement
  if (!host.contains(event.relatedTarget as Node | null)) menuOpen.value = false
}
function onDocumentClick(event: MouseEvent) {
  if (menuRoot.value && !menuRoot.value.contains(event.target as Node)) menuOpen.value = false
}
// Capture phase: closing the menu must stop the global Esc handler (it would clear the selection).
function onDocumentKeydown(event: KeyboardEvent) {
  if (event.key !== 'Escape' || !menuOpen.value) return
  menuOpen.value = false
  event.stopPropagation()
}
onMounted(() => {
  document.addEventListener('click', onDocumentClick)
  document.addEventListener('keydown', onDocumentKeydown, true)
})
onBeforeUnmount(() => {
  document.removeEventListener('click', onDocumentClick)
  document.removeEventListener('keydown', onDocumentKeydown, true)
})

defineExpose({ focusFilter: () => filterInput.value?.focus() })

const updated = computed(() =>
  state.snapshot && state.snapshot.takenAt > 0
    ? new Date(state.snapshot.takenAt).toLocaleTimeString('en-GB')
    : '—',
)
const refreshTitle = computed(() =>
  isNarrow.value ? `Refresh (⌘R) · Updated ${updated.value}` : 'Refresh (⌘R)',
)

const titleSize = computed(() => (isNarrow.value ? 'text-[15px]' : 'text-base'))

watch(railShown, (shown) => {
  if (!shown) menuOpen.value = false
})

const IC = 'h-[15px] w-[15px] shrink-0'
const IB = 'flex h-8 w-8 items-center justify-center rounded-nav text-ink-2 hover:bg-hover'
</script>

<template>
  <header
    data-tauri-drag-region
    :class="[
      'flex h-[60px] shrink-0 items-center gap-2.5 pr-3',
      railShown ? 'pl-[76px]' : 'pl-5',
    ]"
  >
    <div data-tauri-drag-region class="min-w-0">
      <div v-if="!isNarrow" data-testid="eyebrow" data-tauri-drag-region class="text-[11px] leading-none text-ink-3">Services</div>
      <div v-if="railShown" ref="menuRoot" class="relative" @focusout="onFocusOut">
        <h1 :class="['font-semibold tracking-tight', titleSize]">
          <button
            type="button"
            :aria-expanded="menuOpen"
            class="inline-flex items-center gap-1.5"
            @click="menuOpen = !menuOpen"
          >
            <span class="truncate">{{ title }}</span>
            <svg class="h-[13px] w-[13px] text-ink-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m6 9 6 6 6-6" /></svg>
          </button>
        </h1>
        <div
          v-if="menuOpen"
          data-testid="section-list"
          class="absolute left-0 top-full z-20 mt-1 flex min-w-[180px] flex-col rounded-nav border border-line-2 bg-card p-1 shadow-card"
        >
          <button
            v-for="c in choices"
            :key="c.key"
            type="button"
            data-testid="section-option"
            class="rounded-nav px-2.5 py-1 text-left text-[12.5px] hover:bg-hover"
            @click="choose(c.section)"
          >{{ c.label }}</button>
        </div>
      </div>
      <h1 v-else data-tauri-drag-region :class="['truncate font-semibold tracking-tight', titleSize]">{{ title }}</h1>
      <div v-if="group" data-testid="subtitle" data-tauri-drag-region class="truncate font-mono text-[11px] text-ink-3">
        Compose · {{ group.workingDir ?? '—' }}
      </div>
    </div>

    <label
      :class="[
        'ml-3 flex h-8 min-w-0 items-center gap-2 rounded-full border border-line bg-card px-3 text-ink-3',
        isNarrow ? 'min-w-[120px] flex-[1_1_200px]' : 'flex-[0_1_260px]',
      ]"
    >
      <svg :class="IC" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><circle cx="11" cy="11" r="7" /><path d="m20 20-3.5-3.5" /></svg>
      <input
        ref="filterInput"
        type="search"
        aria-label="Filter"
        :placeholder="isNarrow ? 'Filter…' : 'Filter by name, command, image or port'"
        class="min-w-0 flex-1 border-0 bg-transparent text-[12.5px] text-ink outline-none"
        :value="state.filter"
        @input="setFilter(($event.target as HTMLInputElement).value)"
      />
    </label>

    <div data-tauri-drag-region class="flex-1 self-stretch" />

    <template v-if="group">
      <button type="button" :class="BTN_PAGE" @click="runComposeAction(group, 'start')">
        <svg class="h-3 w-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M7 5v14l11-7z" /></svg>Start all
      </button>
      <button type="button" :class="BTN_PAGE" @click="runComposeAction(group, 'stop')">
        <svg class="h-3 w-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="6" y="6" width="12" height="12" rx="1.5" /></svg>Stop all
      </button>
    </template>

    <label class="flex cursor-pointer items-center gap-1.5 text-xs text-ink-2">
      <input
        type="checkbox"
        class="peer sr-only"
        :checked="settings.settings?.showAll ?? false"
        :disabled="settings.settings === null"
        @change="toggleShowAll"
      />
      <span
        aria-hidden="true"
        class="relative inline-block h-4 w-7 rounded-full bg-line-2 transition-colors peer-checked:bg-accent peer-disabled:opacity-40 peer-focus-visible:outline peer-focus-visible:outline-2 peer-focus-visible:outline-accent after:absolute after:left-0.5 after:top-0.5 after:h-3 after:w-3 after:rounded-full after:bg-knob after:transition-transform peer-checked:after:translate-x-3"
      />
      {{ isNarrow ? 'All' : 'Show all' }}
    </label>

    <span v-if="!isNarrow" data-testid="updated" data-tauri-drag-region class="font-mono text-[11.5px] text-ink-3">Updated {{ updated }}</span>

    <button type="button" aria-label="Refresh" :title="refreshTitle" :class="IB" @click="refresh">
      <svg :class="IC" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M21 12a9 9 0 0 1-15.4 6.4L3 16" /><path d="M3 12a9 9 0 0 1 15.4-6.4L21 8" /><path d="M21 3v5h-5" /><path d="M3 21v-5h5" /></svg>
    </button>
    <button type="button" aria-label="Settings" title="Settings (⌘,)" :class="IB" @click="emit('open-settings')">
      <svg :class="IC" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M4 6h9M17 6h3M4 12h3M11 12h9M4 18h11M19 18h1" /><circle cx="15" cy="6" r="2" /><circle cx="9" cy="12" r="2" /><circle cx="17" cy="18" r="2" /></svg>
    </button>
  </header>
  <div
    v-if="state.error"
    role="alert"
    class="mx-3 mb-2 rounded-[10px] border border-danger bg-soft px-4 py-2 text-[12.5px] text-danger"
  >
    Last refresh failed: {{ state.error }}
  </div>
</template>
