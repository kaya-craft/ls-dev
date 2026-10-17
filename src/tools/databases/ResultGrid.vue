<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { toasts } from '../../ui/store/toasts'
import { BTN_ROW } from '../../ui/layout'
import { toCsv, toJson, toMarkdown, toTsv } from './copy'
import { errorText } from './store'
import type { RunError, RunOutput } from './types'

const props = defineProps<{ output: RunOutput | null; error: RunError | null; running: boolean }>()

const MAX_CELL = 200

const result = computed(() => props.output?.result ?? null)
const columns = computed(() => result.value?.columns ?? [])
const rows = computed(() => result.value?.rows ?? [])

function plural(n: number): string {
  return n === 1 ? '1 row' : `${n} rows`
}

const status = computed(() => {
  const out = props.output
  if (!out) return ''
  const time = `${out.elapsedMs} ms`
  if (out.result && out.result.columns.length > 0) return `${plural(out.result.rows.length)} · ${time}`
  const parts = out.statements.map((s) => `${plural(s.rows)} affected`)
  return [...parts, time].join(' · ')
})

function shown(value: string | null): string {
  if (value === null) return 'NULL'
  return value.length > MAX_CELL ? value.slice(0, MAX_CELL) + '…' : value
}

// Selection: a rectangle between an anchor and a focus cell.
interface Cell {
  r: number
  c: number
}
const anchor = ref<Cell | null>(null)
const focus = ref<Cell | null>(null)

watch(result, () => {
  anchor.value = null
  focus.value = null
})

const rect = computed(() => {
  if (!anchor.value || !focus.value) return null
  return {
    r0: Math.min(anchor.value.r, focus.value.r),
    r1: Math.max(anchor.value.r, focus.value.r),
    c0: Math.min(anchor.value.c, focus.value.c),
    c1: Math.max(anchor.value.c, focus.value.c),
  }
})

function isSelected(r: number, c: number): boolean {
  const q = rect.value
  return !!q && r >= q.r0 && r <= q.r1 && c >= q.c0 && c <= q.c1
}

function clickCell(event: MouseEvent, r: number, c: number) {
  if (event.shiftKey && anchor.value) focus.value = { r, c }
  else {
    anchor.value = { r, c }
    focus.value = { r, c }
  }
}

function clickRow(r: number) {
  anchor.value = { r, c: 0 }
  focus.value = { r, c: Math.max(columns.value.length - 1, 0) }
}

async function writeClipboard(text: string) {
  try {
    await navigator.clipboard.writeText(text)
    toasts.success('Copied')
  } catch (e) {
    toasts.error(`Copy failed: ${e instanceof Error ? e.message : String(e)}`)
  }
}

function onKeydown(event: KeyboardEvent) {
  if (!(event.metaKey || event.ctrlKey) || event.key.toLowerCase() !== 'c') return
  const q = rect.value
  if (!q) return
  event.preventDefault()
  const selectedRows = rows.value.slice(q.r0, q.r1 + 1).map((row) => row.slice(q.c0, q.c1 + 1))
  if (selectedRows.length === 1 && selectedRows[0].length === 1) {
    void writeClipboard(selectedRows[0][0] ?? '')
  } else {
    void writeClipboard(toTsv(columns.value.slice(q.c0, q.c1 + 1), selectedRows, false))
  }
}

// "Copy as" menu.
const menuOpen = ref(false)
const menuRoot = ref<HTMLElement | null>(null)
const FORMATS = [
  { id: 'tsv', label: 'TSV', fn: toTsv },
  { id: 'csv', label: 'CSV', fn: toCsv },
  { id: 'json', label: 'JSON', fn: toJson },
  { id: 'markdown', label: 'Markdown', fn: toMarkdown },
] as const

function copyAs(fn: (columns: string[], rows: (string | null)[][]) => string) {
  menuOpen.value = false
  void writeClipboard(fn(columns.value, rows.value))
}

function onDocumentClick(event: MouseEvent) {
  if (menuOpen.value && !(menuRoot.value && menuRoot.value.contains(event.target as Node))) menuOpen.value = false
}
// Capture phase: closing the menu must stop other Escape handlers.
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
</script>

<template>
  <div class="relative flex h-full min-h-0 flex-col overflow-hidden rounded-card border border-line bg-card">
    <div v-if="error && !running" data-testid="error" class="whitespace-pre-wrap p-4 text-[12.5px] text-danger">{{ errorText(error) }}</div>
    <div v-else-if="!output" class="flex flex-1 items-center justify-center p-4 text-xs text-ink-3">
      <span v-if="!running">Run a query to see its result here.</span>
    </div>
    <template v-else>
      <div v-if="output.rolledBack" data-testid="banner" class="border-b border-line px-3 py-1.5 text-xs text-ink-2">
        Earlier statements in this batch were rolled back.
      </div>
      <div v-if="result?.truncated" data-testid="banner" class="border-b border-line px-3 py-1.5 text-xs text-ink-2">
        Showing the first 1,000 rows; the query was stopped. Changes made by this batch, if any, were undone.
      </div>
      <div
        v-if="result && columns.length > 0"
        data-testid="grid"
        tabindex="0"
        class="min-h-0 flex-1 overflow-auto outline-none"
        @keydown="onKeydown"
      >
        <table class="border-separate border-spacing-0 font-mono text-xs">
          <thead>
            <tr>
              <th class="sticky left-0 top-0 z-20 border-b border-line bg-card px-2 py-1" />
              <th
                v-for="(name, c) in columns"
                :key="c"
                class="sticky top-0 z-10 whitespace-nowrap border-b border-line bg-card px-2 py-1 text-left font-medium text-ink-2"
              >
                {{ name }}
              </th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="(row, r) in rows" :key="r">
              <td
                data-testid="row-number"
                class="sticky left-0 z-[1] cursor-pointer select-none border-b border-line bg-card px-2 py-0.5 text-right tabular-nums text-ink-3 hover:bg-hover"
                @click="clickRow(r)"
              >
                {{ r + 1 }}
              </td>
              <td
                v-for="(value, c) in row"
                :key="c"
                data-testid="cell"
                :title="value !== null && value.length > MAX_CELL ? value : undefined"
                :class="[
                  'max-w-[480px] cursor-default select-none whitespace-nowrap border-b border-line px-2 py-0.5',
                  value === null ? 'italic text-ink-3' : '',
                  isSelected(r, c) ? 'bg-accent-soft' : '',
                ]"
                @click="clickCell($event, r, c)"
              >
                {{ shown(value) }}
              </td>
            </tr>
          </tbody>
        </table>
      </div>
      <div v-else class="flex-1" />
      <div class="flex items-center gap-2 border-t border-line px-3 py-1.5">
        <span data-testid="status" class="text-xs tabular-nums text-ink-3">{{ status }}</span>
        <span class="flex-1" />
        <span v-if="result && columns.length > 0" ref="menuRoot" class="relative">
          <button type="button" data-testid="copy-as" :class="BTN_ROW" aria-haspopup="menu" :aria-expanded="menuOpen" @click="menuOpen = !menuOpen">
            Copy as
          </button>
          <span v-if="menuOpen" class="absolute bottom-full right-0 z-20 mb-1 flex min-w-[120px] flex-col rounded-[10px] border border-line-2 bg-card p-1 shadow-card">
            <button
              v-for="f in FORMATS"
              :key="f.id"
              type="button"
              :data-testid="`copy-${f.id}`"
              class="rounded-[6px] px-2.5 py-1 text-left text-xs hover:bg-hover"
              @click="copyAs(f.fn)"
            >
              {{ f.label }}
            </button>
          </span>
        </span>
      </div>
    </template>
    <div v-if="running" class="absolute inset-0 z-30 flex items-center justify-center gap-2 bg-card/70 text-xs text-ink-2">
      <span aria-hidden="true" class="size-3.5 animate-spin rounded-full border-2 border-line-2 border-t-accent" />
      Running…
    </div>
  </div>
</template>
