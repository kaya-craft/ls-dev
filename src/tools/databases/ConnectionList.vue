<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from 'vue'
import { BTN_PAGE } from '../../ui/layout'
import { confirm } from '../../ui/store/confirm'
import { toasts } from '../../ui/store/toasts'
import { api as servicesApi } from '../services/api'
import { databasesApi } from './api'
import { connectionColor, useDatabases } from './store'
import type { DetectedConnection, SavedConnection } from './types'

const emit = defineEmits<{
  add: []
  edit: [connection: SavedConnection]
  'save-detected': [detected: DetectedConnection]
}>()

const { state, refresh, select } = useDatabases()
const SMALL = 'h-7 px-2.5 text-xs'

function message(e: unknown): string {
  return e instanceof Error ? e.message : String(e)
}

const menuFor = ref<string | null>(null)
const menuRoots = new Map<string, HTMLElement>()
function setMenuRoot(id: string, el: unknown) {
  if (el instanceof HTMLElement) menuRoots.set(id, el)
  else menuRoots.delete(id)
}
function onDocumentClick(event: MouseEvent) {
  const root = menuFor.value === null ? undefined : menuRoots.get(menuFor.value)
  if (menuFor.value !== null && !(root && root.contains(event.target as Node))) menuFor.value = null
}
// Capture phase: closing the menu must stop the global Esc handler.
function onDocumentKeydown(event: KeyboardEvent) {
  if (event.key !== 'Escape' || menuFor.value === null) return
  menuFor.value = null
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

function edit(c: SavedConnection) {
  menuFor.value = null
  emit('edit', c)
}

async function remove(c: SavedConnection) {
  menuFor.value = null
  const answer = await confirm({
    title: `Delete connection ${c.name}?`,
    body: 'The saved password is removed from the keychain.',
    confirmLabel: 'Delete',
    danger: true,
  })
  if (!answer.ok) return
  try {
    await databasesApi.remove(c.id)
    await refresh()
  } catch (e) {
    toasts.error(message(e))
  }
}

function saveDetected(d: DetectedConnection) {
  menuFor.value = null
  emit('save-detected', d)
}

const starting = ref<string | null>(null)
async function start(d: DetectedConnection) {
  starting.value = d.id
  try {
    await servicesApi.containerAction(`docker:${d.containerId}`, 'start')
    await refresh()
  } catch (e) {
    toasts.error(message(e))
  } finally {
    starting.value = null
  }
}
</script>

<template>
  <div class="flex flex-col">
    <div class="flex items-center px-1.5 pb-2 text-xs font-semibold">
      <span class="flex-1">Connections</span>
      <button type="button" aria-label="Add connection" class="flex h-6 w-6 items-center justify-center rounded-nav text-ink-2 hover:bg-hover" @click="emit('add')">
        <svg class="h-[13px] w-[13px]" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"><path d="M12 5v14M5 12h14" /></svg>
      </button>
    </div>

    <div v-if="!state.view || state.view.saved.length === 0" class="mx-1.5 mb-3.5 rounded-[10px] border border-dashed border-line-2 p-3 text-center text-xs text-ink-3">No saved connections</div>
    <template v-else>
      <div
        v-for="c in state.view.saved"
        :key="c.id"
        data-testid="saved-row"
        :aria-current="state.activeId === c.id ? 'true' : undefined"
        :class="['flex cursor-default items-center gap-2.5 rounded-[10px] px-2.5 py-2 hover:bg-hover', state.activeId === c.id ? 'bg-hover' : '']"
        @click="select(c.id)"
      >
        <span data-testid="dot" class="h-2.5 w-2.5 shrink-0 rounded-full" :style="{ background: connectionColor(c) }" />
        <div class="min-w-0 flex-1">
          <div class="flex items-center gap-1.5">
            <span class="truncate text-[13px] font-medium">{{ c.name }}</span>
            <span v-if="c.env" class="shrink-0 rounded bg-soft px-1.5 text-[10.5px] text-ink-2">{{ c.env }}</span>
            <svg v-if="c.readOnly" data-testid="lock" aria-label="Read-only" class="h-3 w-3 shrink-0 text-ink-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><rect x="5" y="11" width="14" height="9" rx="2" /><path d="M8 11V8a4 4 0 0 1 8 0v3" /></svg>
          </div>
          <div class="truncate font-mono text-[11px] text-ink-3">{{ c.host }}:{{ c.port }} · {{ c.database }}</div>
        </div>
        <span :ref="(el) => setMenuRoot(c.id, el)" class="relative" @click.stop>
          <button
            type="button"
            :aria-label="`Menu ${c.name}`"
            class="flex h-6 w-6 items-center justify-center rounded-nav text-ink-2 hover:bg-hover"
            @click="menuFor = menuFor === c.id ? null : c.id"
          >
            <svg class="h-[14px] w-[14px]" viewBox="0 0 24 24" fill="currentColor"><circle cx="5" cy="12" r="1.7" /><circle cx="12" cy="12" r="1.7" /><circle cx="19" cy="12" r="1.7" /></svg>
          </button>
          <span v-if="menuFor === c.id" class="absolute right-0 top-full z-20 mt-1 flex min-w-[140px] flex-col rounded-[10px] border border-line-2 bg-card p-1 shadow-card">
            <button type="button" data-action="edit" class="rounded-[6px] px-2.5 py-1 text-left text-xs hover:bg-hover" @click="edit(c)">Edit</button>
            <button type="button" data-action="delete" class="rounded-[6px] px-2.5 py-1 text-left text-xs text-danger hover:bg-hover" @click="remove(c)">Delete</button>
          </span>
        </span>
      </div>
    </template>

    <div class="px-1.5 pb-1.5 pt-1 text-[11px] font-medium text-ink-3">Detected on this machine</div>
    <div v-if="!state.view" class="px-1.5 text-xs text-ink-3">Collecting…</div>
    <div v-else-if="state.view.detected.length === 0" class="px-1.5 text-xs text-ink-3">{{ state.view.dockerAvailable ? 'No Postgres found' : 'Docker is not available' }}</div>
    <div
      v-for="d in state.view?.detected ?? []"
      :key="d.id"
      data-testid="detected"
      :aria-current="state.activeId === d.id ? 'true' : undefined"
      :class="['flex cursor-default items-center gap-2.5 rounded-[10px] px-2.5 py-2 hover:bg-hover', d.running ? '' : 'opacity-55', state.activeId === d.id ? 'bg-hover' : '']"
      @click="d.running && select(d.id)"
    >
      <span :class="['inline-flex h-7 w-7 shrink-0 items-center justify-center rounded-full text-[11px] font-semibold', d.running ? 'bg-p-pg-bg text-p-pg' : 'bg-p-off-bg text-p-off']">Pg</span>
      <div class="min-w-0 flex-1">
        <div :class="['truncate text-[13px]', d.running ? 'font-medium' : '']">{{ d.name }}</div>
        <div class="truncate font-mono text-[11px] text-ink-3">
          {{ [d.project, d.port !== null ? `:${d.port}` : '', d.running ? '' : 'stopped'].filter(Boolean).join(' · ') }}
        </div>
      </div>
      <button v-if="!d.running" type="button" data-action="start" :disabled="starting === d.id" :class="[BTN_PAGE, SMALL]" @click.stop="start(d)">Start</button>
      <span :ref="(el) => setMenuRoot(d.id, el)" class="relative" @click.stop>
        <button
          type="button"
          :aria-label="`Menu ${d.name}`"
          class="flex h-6 w-6 items-center justify-center rounded-nav text-ink-2 hover:bg-hover"
          @click="menuFor = menuFor === d.id ? null : d.id"
        >
          <svg class="h-[14px] w-[14px]" viewBox="0 0 24 24" fill="currentColor"><circle cx="5" cy="12" r="1.7" /><circle cx="12" cy="12" r="1.7" /><circle cx="19" cy="12" r="1.7" /></svg>
        </button>
        <span v-if="menuFor === d.id" class="absolute right-0 top-full z-20 mt-1 flex min-w-[160px] flex-col rounded-[10px] border border-line-2 bg-card p-1 shadow-card">
          <button type="button" data-action="save-detected" class="rounded-[6px] px-2.5 py-1 text-left text-xs hover:bg-hover" @click="saveDetected(d)">Save as connection…</button>
        </span>
      </span>
    </div>
  </div>
</template>
