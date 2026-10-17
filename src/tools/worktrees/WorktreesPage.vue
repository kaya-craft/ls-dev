<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, onUnmounted, ref } from 'vue'
import PageHeader from '../../ui/PageHeader.vue'
import { BTN_PAGE, BTN_PRIMARY, BTN_ROW, BTN_ROW_DANGER } from '../../ui/layout'
import { formatAgo, formatBytes } from '../../ui/format'
import { setServicesSection, setTool } from '../../shell/navigation'
import { select, setFilter } from '../services/store/services'
import { splitName } from '../services/rowText'
import type { ProcessService } from '../services/types'
import { addFolder, excludeRepo, openShell, removeAllMerged, removeWorktree, rescan } from './actions'
import { setSort, useWorktrees, type WorktreeSort } from './store'
import type { RepoGroup, WorktreeInfo } from './types'

defineEmits<{ 'open-settings': [] }>()

const { state, groups, summary, eligibleMerged, servicesIn } = useWorktrees()

const SCAN_INTERVAL_MS = 60_000
const nowSec = ref(Math.floor(Date.now() / 1000))
let timer: ReturnType<typeof setInterval> | undefined
function tick() {
  nowSec.value = Math.floor(Date.now() / 1000)
  void rescan({ silent: true })
}
onMounted(() => {
  void rescan({ silent: true })
  timer = setInterval(tick, SCAN_INTERVAL_MS)
})
onUnmounted(() => clearInterval(timer))

const scanning = computed(() => state.snapshot?.scanning === true)
const failed = computed(() => state.snapshot === null && state.error !== null)
const waiting = computed(() => (state.snapshot === null && !failed.value) || (scanning.value && groups.value.length === 0))
const empty = computed(() => state.snapshot !== null && !waiting.value && groups.value.length === 0)

const GRID = 'grid grid-cols-[minmax(0,1.5fr)_84px_110px_minmax(0,1.6fr)] items-center gap-4 px-4'
const GHOST_GRID = 'grid grid-cols-[minmax(0,1.6fr)_minmax(0,1.2fr)_80px_110px_120px] items-center gap-4 px-4'
const GHOST_ROWS = [
  { opacity: 'opacity-90', branch: 'w-[110px]', repo: 'w-[90px]', size: 'w-11', date: 'w-16', running: true },
  { opacity: 'opacity-60', branch: 'w-20', repo: 'w-[70px]', size: 'w-[38px]', date: 'w-[54px]', running: false },
  { opacity: 'opacity-30', branch: 'w-[130px]', repo: 'w-[90px]', size: 'w-11', date: 'w-[60px]', running: false },
]
const SORTS: { id: WorktreeSort; label: string }[] = [
  { id: 'size', label: 'Size' },
  { id: 'date', label: 'Last commit' },
]

const BADGE = 'whitespace-nowrap rounded px-1.5 font-mono text-[11px]'
const UNTRUSTED_TITLE =
  'The Git configuration of this repository is not trusted: LsDev did not read the local state of this worktree (changes, upstream, merge status).'

const plural = (n: number, word: string) => `${n} ${word}${n === 1 ? '' : 's'}`
const summaryText = computed(() => {
  const s = summary.value
  return `${plural(s.count, 'worktree')} · ${formatBytes(s.totalBytes)} · ${s.withoutService} without running service`
})

type Badge = { text: string; tone: string; title?: string; testid?: string }
function badges(w: WorktreeInfo): Badge[] {
  if (!w.trusted) return [{ text: 'untrusted config', tone: 'bg-soft text-danger', title: UNTRUSTED_TITLE, testid: 'untrusted-badge' }]
  const out: Badge[] = []
  if (w.changes === null) out.push({ text: 'state unknown', tone: 'bg-soft text-ink-3', title: 'git status failed for this worktree' })
  else if (w.changes > 0) out.push({ text: plural(w.changes, 'change'), tone: 'bg-soft text-warm' })
  if (w.branch !== null) {
    if (w.upstream.kind === 'tracking' && w.upstream.ahead > 0) out.push({ text: `${w.upstream.ahead} unpushed`, tone: 'bg-soft text-warm' })
    else if (w.upstream.kind === 'none') out.push({ text: 'no upstream', tone: 'bg-soft text-ink-3' })
    else if (w.upstream.kind === 'gone') out.push({ text: 'upstream gone', tone: 'bg-soft text-ink-3' })
  }
  if (w.merged) out.push({ text: 'merged', tone: 'bg-accent-soft text-accent' })
  return out
}

function ago(w: WorktreeInfo): string {
  return w.lastCommitAt === null ? '—' : formatAgo(nowSec.value - w.lastCommitAt)
}

function openService(s: ProcessService) {
  setTool('services')
  setServicesSection('all')
  setFilter('')
  select(s.id)
}

const isRemoving = (w: WorktreeInfo) => state.removing.includes(w.path)

function remove(repo: RepoGroup, w: WorktreeInfo) {
  void removeWorktree({ branch: w.branch ?? '(detached)', path: w.path, mainPath: repo.path }, servicesIn(w.path))
}

const menuFor = ref<string | null>(null)
const menuRoots = new Map<string, HTMLElement>()
function setMenuRoot(path: string, el: unknown) {
  if (el instanceof HTMLElement) menuRoots.set(path, el)
  else menuRoots.delete(path)
}
function onDocumentClick(event: MouseEvent) {
  const root = menuFor.value === null ? undefined : menuRoots.get(menuFor.value)
  if (menuFor.value !== null && !(root && root.contains(event.target as Node))) menuFor.value = null
}
// Capture phase: closing the menu must stop the global Esc handler (it would clear the selection).
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
function exclude(repo: RepoGroup) {
  menuFor.value = null
  void excludeRepo(repo.path)
}
</script>

<template>
  <div class="flex min-h-0 flex-1 flex-col">
    <PageHeader title="Worktrees" subtitle="Git worktrees across your local repositories" @open-settings="$emit('open-settings')">
      <template #action>
        <button type="button" :class="[BTN_PAGE, 'h-[30px]']" :disabled="scanning" @click="rescan()">Rescan</button>
        <button type="button" :class="[BTN_PAGE, 'h-[30px]']" :disabled="eligibleMerged.length === 0 || state.removing.length > 0" @click="removeAllMerged">
          Remove all merged{{ eligibleMerged.length > 0 ? ` (${eligibleMerged.length})` : '' }}
        </button>
        <button type="button" :class="[BTN_PAGE, 'h-[30px]']" @click="addFolder">
          <svg class="h-[13px] w-[13px]" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"><path d="M12 5v14M5 12h14" /></svg>
          Add folder
        </button>
      </template>
    </PageHeader>

    <div v-if="state.error" role="alert" class="mx-4 mb-2 rounded-tile border border-danger px-3 py-1.5 text-xs text-danger">{{ state.error }}</div>

    <div v-if="!waiting && !empty && !failed" class="mx-4 mb-2 flex items-center gap-3 text-xs text-ink-2">
      <span data-testid="summary" class="font-mono tabular-nums">{{ summaryText }}</span>
      <span class="flex-1" />
      <span class="text-ink-3">Sort</span>
      <div role="group" aria-label="Sort" class="flex rounded-nav border border-line-2 bg-card p-0.5">
        <button
          v-for="s in SORTS"
          :key="s.id"
          type="button"
          :aria-pressed="state.sort === s.id"
          :class="['rounded-[7px] px-2.5 py-0.5 text-xs', state.sort === s.id ? 'bg-active text-ink' : 'text-ink-2 hover:bg-hover']"
          @click="setSort(s.id)"
        >{{ s.label }}</button>
      </div>
    </div>

    <div v-if="failed" class="mx-4 mb-4 flex flex-1 items-center justify-center rounded-tile border border-line bg-card text-[13px] text-ink-3 shadow-card">
      Scan failed. Use Rescan to try again.
    </div>

    <div v-else-if="waiting" class="mx-4 mb-4 flex flex-1 items-center justify-center rounded-tile border border-line bg-card text-[13px] text-ink-3 shadow-card">
      Scanning…
    </div>

    <div v-else-if="!empty" class="mx-4 mb-4 min-h-0 flex-1 overflow-auto rounded-tile border border-line bg-card shadow-card">
      <div :class="[GRID, 'sticky top-0 z-10 h-[34px] border-b border-line bg-card text-[11.5px] font-medium text-ink-3']">
        <span>Branch</span><span class="text-right">Disk size</span><span class="text-right">Last commit</span><span>State</span>
      </div>
      <section v-for="repo in groups" :key="repo.path" data-testid="repo-group">
        <header class="flex h-[38px] items-center gap-2 border-b border-line bg-soft px-4 text-[12.5px]">
          <span class="font-semibold">{{ repo.name }}</span>
          <span class="min-w-0 truncate font-mono text-[11px] text-ink-3" :title="repo.path">{{ repo.path }}</span>
          <span class="flex-1" />
          <span class="font-mono text-xs tabular-nums text-ink-2" title="Main checkout size">{{ repo.mainSize === null ? '…' : formatBytes(repo.mainSize) }}</span>
          <span :ref="(el) => setMenuRoot(repo.path, el)" class="relative">
            <button
              type="button"
              :aria-label="`Menu ${repo.name}`"
              class="flex h-6 w-6 items-center justify-center rounded-nav text-ink-2 hover:bg-hover"
              @click="menuFor = menuFor === repo.path ? null : repo.path"
            >
              <svg class="h-[14px] w-[14px]" viewBox="0 0 24 24" fill="currentColor"><circle cx="5" cy="12" r="1.7" /><circle cx="12" cy="12" r="1.7" /><circle cx="19" cy="12" r="1.7" /></svg>
            </button>
            <span
              v-if="menuFor === repo.path"
              class="absolute right-0 top-full z-20 mt-1 flex min-w-[160px] flex-col rounded-[10px] border border-line-2 bg-card p-1 shadow-card"
              @mouseleave="menuFor = null"
            >
              <button type="button" class="rounded-[6px] px-2.5 py-1 text-left text-xs hover:bg-hover" @click="exclude(repo)">Exclude repository</button>
            </span>
          </span>
        </header>
        <div
          v-for="w in repo.worktrees"
          :key="w.path"
          data-testid="worktree-row"
          :data-path="w.path"
          :aria-busy="isRemoving(w)"
          :class="[GRID, 'group relative h-[50px] cursor-default border-b border-line text-sm hover:bg-row-hover', isRemoving(w) ? 'text-ink-3' : '']"
        >
          <div class="min-w-0">
            <div class="flex items-center gap-1.5">
              <span class="min-w-0 truncate rounded bg-branch-bg px-1.5 font-mono text-[12px] text-branch">{{ w.branch ?? 'detached' }}</span>
            </div>
            <div class="mt-0.5 truncate font-mono text-[11px] text-ink-3" :title="w.path">{{ w.path }}</div>
          </div>
          <div class="text-right font-mono text-xs tabular-nums text-ink-2">{{ w.sizeBytes === null ? '…' : formatBytes(w.sizeBytes) }}</div>
          <div class="text-right font-mono text-xs tabular-nums text-ink-2">{{ ago(w) }}</div>
          <div class="flex min-w-0 flex-wrap items-center gap-1 overflow-hidden">
            <span
              v-for="b in badges(w)"
              :key="b.text"
              :data-testid="b.testid"
              :title="b.title"
              :class="[BADGE, b.tone]"
            >{{ b.text }}</span>
            <button
              v-for="s in servicesIn(w.path)"
              :key="s.id"
              type="button"
              data-testid="service-chip"
              :title="`Open ${s.project} in Services`"
              :class="[BADGE, 'bg-accent-soft text-accent hover:underline']"
              @click="openService(s)"
            >{{ splitName(s.project).last }}</button>
          </div>
          <div
            v-if="isRemoving(w)"
            data-testid="removing"
            role="status"
            class="absolute inset-y-0 right-3 flex items-center gap-2 bg-row-hover pl-3 text-xs text-ink-2"
          >
            <span aria-hidden="true" class="size-3.5 animate-spin rounded-full border-2 border-line-2 border-t-danger" />
            Removing…
          </div>
          <div v-else class="pointer-events-none absolute inset-y-0 right-3 flex items-center justify-end gap-1 bg-row-hover pl-3 opacity-0 group-focus-within:opacity-100 group-hover:opacity-100 [&_*]:pointer-events-auto">
            <button type="button" data-testid="open-shell" :class="BTN_ROW" :title="`Open a terminal in ${w.path}`" @click="openShell(w.path)">Open</button>
            <button type="button" data-testid="remove" :class="BTN_ROW_DANGER" @click="remove(repo, w)">Remove</button>
          </div>
        </div>
      </section>
    </div>

    <div v-else class="mx-4 mb-4 flex min-h-0 flex-1 flex-col overflow-hidden rounded-tile border border-line bg-card shadow-card">
      <div aria-hidden="true" class="shrink-0 pt-1">
        <div :class="[GHOST_GRID, 'h-[34px] border-b border-line text-[11.5px] font-medium text-ink-3']">
          <span>Branch</span><span>Repository</span><span class="text-right">Disk size</span><span class="text-right">Last modified</span><span>Running</span>
        </div>
        <div v-for="(r, i) in GHOST_ROWS" :key="i" :class="[GHOST_GRID, 'h-[46px]', r.opacity, i < GHOST_ROWS.length - 1 ? 'border-b border-line' : '']">
          <span class="flex items-center gap-2">
            <span class="h-[18px] w-[18px] rounded-md bg-branch-bg" />
            <span :class="['h-2 rounded-full bg-track', r.branch]" />
          </span>
          <span :class="['h-2 rounded-full bg-track', r.repo]" />
          <span :class="['h-2 justify-self-end rounded-full bg-track', r.size]" />
          <span :class="['h-2 justify-self-end rounded-full bg-track', r.date]" />
          <span class="flex gap-1"><span v-if="r.running" class="h-4 w-10 rounded-[5px] bg-accent-soft" /></span>
        </div>
      </div>

      <div class="flex flex-1 items-center justify-center p-6">
        <div class="flex max-w-[440px] flex-col items-center text-center">
          <span aria-hidden="true" class="inline-flex h-[52px] w-[52px] items-center justify-center rounded-2xl bg-branch-bg text-branch">
            <svg class="h-6 w-6" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round">
              <circle cx="6" cy="5" r="2" /><circle cx="6" cy="19" r="2" /><circle cx="18" cy="7" r="2" /><path d="M6 7v10M18 9c0 4-3 6-12 8" />
            </svg>
          </span>
          <h2 class="mb-1.5 mt-4 text-[17px] font-semibold tracking-tight">No repositories to scan yet</h2>
          <p class="text-[13px] text-ink-2">Pick the folders where your code lives. LsDev lists every git worktree inside them, with its branch, size on disk, last change and the services running from it.</p>
          <div class="mt-[18px] flex gap-2">
            <button type="button" :class="BTN_PRIMARY" @click="addFolder">Choose a folder…</button>
          </div>
          <p class="mt-[18px] text-[11.5px] text-ink-3">
            New to worktrees?
            <span class="rounded-[5px] border border-line bg-soft px-1.5 py-px font-mono text-ink-2">git worktree add ../app-login feat/login</span>
          </p>
        </div>
      </div>
    </div>
  </div>
</template>
