<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import PageHeader from '../../ui/PageHeader.vue'
import { BTN_PAGE, BTN_PRIMARY } from '../../ui/layout'
import { confirm } from '../../ui/store/confirm'
import { toasts } from '../../ui/store/toasts'
import { useServices } from '../services/store/services'
import { databasesApi } from './api'
import ConnectionDialog from './ConnectionDialog.vue'
import ConnectionList from './ConnectionList.vue'
import ResultGrid from './ResultGrid.vue'
import SqlEditor from './SqlEditor.vue'
import { connectionColor, useDatabases } from './store'
import type { ConnectionDraft, DetectedConnection, SavedConnection } from './types'

defineEmits<{ 'open-settings': [] }>()

const { state, active, draft, output, error, running, refresh, setDraft, run, cancel, setReadOnly } = useDatabases()
const services = useServices()

const SMALL = 'h-7 px-2.5 text-xs'
const CARD = 'rounded-tile border border-line bg-card shadow-card'
const REFRESH_INTERVAL = 5000

function message(e: unknown): string {
  return e instanceof Error ? e.message : String(e)
}

const empty = computed(() => !state.view || (state.view.saved.length === 0 && state.view.detected.length === 0))
const env = computed(() => active.value?.saved?.env ?? null)
const color = computed(() => (active.value ? connectionColor({ color: active.value.color, env: env.value }) : null))

// List refresh: on mount, then when the Services snapshot changes, at most once per 5 s.
let lastRefresh = 0
let pending: ReturnType<typeof setTimeout> | null = null
async function doRefresh(report: boolean) {
  lastRefresh = Date.now()
  try {
    await refresh()
  } catch (e) {
    if (report) toasts.error(message(e))
  }
}
function throttledRefresh() {
  if (pending !== null) return
  const wait = lastRefresh + REFRESH_INTERVAL - Date.now()
  if (wait <= 0) {
    void doRefresh(false)
    return
  }
  pending = setTimeout(() => {
    pending = null
    void doRefresh(false)
  }, wait)
}
onMounted(() => void doRefresh(true))
onBeforeUnmount(() => {
  if (pending !== null) clearTimeout(pending)
})
watch(() => services.state.snapshot?.takenAt, throttledRefresh)

// History of the active connection, reloaded on selection and after each run.
const history = ref<string[]>([])
async function loadHistory(id: string | null) {
  if (!id) {
    history.value = []
    return
  }
  try {
    const list = await databasesApi.history(id)
    if (state.activeId === id) history.value = list
  } catch {
    if (state.activeId === id) history.value = []
  }
}
// Emptied first so ⌘↑ never recalls a query of the previous connection.
watch(
  () => active.value?.id ?? null,
  (id) => {
    history.value = []
    void loadHistory(id)
  },
  { immediate: true },
)

async function clearHistory() {
  const a = active.value
  if (!a) return
  const answer = await confirm({
    title: `Clear the query history of ${a.name}?`,
    body: 'Queries recalled with ⌘↑ in this connection are removed.',
    confirmLabel: 'Clear',
    danger: true,
  })
  if (!answer.ok) return
  try {
    await databasesApi.clearHistory(a.id)
    if (state.activeId === a.id) history.value = []
  } catch (e) {
    toasts.error(message(e))
  }
}

const sql = computed({
  get: () => draft.value,
  set: (value: string) => {
    if (state.activeId) setDraft(state.activeId, value)
  },
})

async function runQuery() {
  const id = active.value?.id
  if (!id || running.value) return
  await run()
  void loadHistory(id)
}

const editor = ref<InstanceType<typeof SqlEditor> | null>(null)
watch(
  error,
  (e) => editor.value?.markErrorLine(e?.kind === 'db' ? e.position : null),
  { flush: 'post' },
)

const toggling = ref(false)
async function toggleReadOnly() {
  const a = active.value
  if (!a || toggling.value) return
  toggling.value = true
  try {
    await setReadOnly(a.id, !a.readOnly)
  } catch (e) {
    toasts.error(message(e))
  } finally {
    toggling.value = false
  }
}

const dialogOpen = ref(false)
const dialogInitial = ref<ConnectionDraft | null>(null)
function openDialog(initial: ConnectionDraft | null) {
  dialogInitial.value = initial
  dialogOpen.value = true
}
function editConnection(c: SavedConnection) {
  openDialog({ ...c })
}
function saveDetected(d: DetectedConnection) {
  openDialog({
    id: null,
    name: d.name,
    env: null,
    color: null,
    host: '127.0.0.1',
    port: d.port ?? 5432,
    user: 'postgres',
    database: '',
    sslMode: 'disable',
    readOnly: false,
  })
}
</script>

<template>
  <div class="flex min-h-0 flex-1 flex-col">
    <PageHeader title="Databases" subtitle="Postgres connections" @open-settings="$emit('open-settings')">
      <template #action>
        <button type="button" :class="[BTN_PAGE, 'h-[30px]']" @click="openDialog(null)">
          <svg class="h-[13px] w-[13px]" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"><path d="M12 5v14M5 12h14" /></svg>
          Add connection
        </button>
      </template>
    </PageHeader>

    <div class="flex min-h-0 flex-1 gap-3 px-4 pb-4">
      <section aria-label="Connections" :class="[CARD, 'flex w-[250px] shrink-0 flex-col overflow-auto px-2 py-3']">
        <ConnectionList @add="openDialog(null)" @edit="editConnection" @save-detected="saveDetected" />
      </section>

      <div class="flex min-w-0 flex-1 flex-col gap-3">
        <section aria-label="Query" :class="[CARD, 'flex min-w-0 flex-col overflow-hidden', empty ? 'flex-1' : 'shrink-0']">
          <div class="flex shrink-0 items-center gap-2 border-b border-line px-3 py-2.5">
            <template v-if="active">
              <span data-testid="active-dot" class="h-2.5 w-2.5 shrink-0 rounded-full" :style="{ background: color! }" />
              <span class="truncate text-[13px] font-medium">{{ active.name }}</span>
              <span v-if="env" class="shrink-0 rounded bg-soft px-1.5 text-[10.5px] text-ink-2">{{ env }}</span>
            </template>
            <span v-else class="inline-flex h-7 items-center gap-[7px] rounded-lg border border-line-2 px-2.5 text-xs text-ink-3">
              <svg class="h-[13px] w-[13px]" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><ellipse cx="12" cy="5.5" rx="7.5" ry="2.5" /><path d="M4.5 5.5v13c0 1.4 3.4 2.5 7.5 2.5s7.5-1.1 7.5-2.5v-13" /></svg>
              No connection
            </span>
            <div class="flex-1" />
            <template v-if="active">
              <button
                type="button"
                aria-label="Clear history"
                title="Clear history"
                :disabled="history.length === 0"
                class="flex h-7 w-7 shrink-0 items-center justify-center rounded-nav text-ink-2 hover:bg-hover disabled:opacity-40 disabled:hover:bg-transparent"
                @click="clearHistory"
              >
                <svg class="h-[14px] w-[14px]" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M3 12a9 9 0 1 0 3-6.7L3 8" /><path d="M3 3v5h5" /><path d="M12 7v5l3 2" /></svg>
              </button>
              <span id="db-readonly-label" class="text-xs text-ink-2">Read-only</span>
              <button
                type="button"
                role="switch"
                data-testid="read-only"
                aria-labelledby="db-readonly-label"
                :aria-checked="active.readOnly ? 'true' : 'false'"
                :disabled="toggling"
                :class="['relative h-5 w-9 shrink-0 rounded-full border border-line-2 transition-colors disabled:opacity-60', active.readOnly ? 'bg-accent' : 'bg-soft']"
                @click="toggleReadOnly"
              >
                <span :class="['absolute top-[1px] h-4 w-4 rounded-full bg-card shadow transition-all', active.readOnly ? 'left-[17px]' : 'left-[1px]']" />
              </button>
            </template>
            <button v-if="running" type="button" :class="[BTN_PAGE, SMALL]" @click="cancel()">Cancel</button>
            <button v-else type="button" :disabled="!active" :class="[BTN_PRIMARY, SMALL]" @click="runQuery">Run <span class="font-mono text-[11px] font-normal">⌘↵</span></button>
          </div>

          <div
            v-if="active"
            data-testid="editor-frame"
            class="h-[200px] shrink-0 border-l-[3px] border-solid"
            :style="{ borderLeftColor: color! }"
          >
            <SqlEditor ref="editor" v-model="sql" :history="history" @run="runQuery" />
          </div>
          <div v-else aria-hidden="true" :class="['h-[132px] shrink-0 bg-soft py-2.5 font-mono text-xs leading-5 text-ink-3', empty ? 'border-b border-line' : '']">
            <div class="flex gap-3.5 pr-3"><span class="w-[22px] shrink-0 text-right text-idle">1</span><span class="italic">-- Pick a connection to start writing SQL</span></div>
            <div v-for="n in 4" :key="n" class="flex gap-3.5"><span class="w-[22px] shrink-0 text-right text-idle">{{ n + 1 }}</span><span /></div>
          </div>

          <div v-if="empty" class="flex flex-1 items-center justify-center p-6">
            <div class="flex max-w-[400px] flex-col items-center text-center">
              <span aria-hidden="true" class="inline-flex h-[52px] w-[52px] items-center justify-center rounded-2xl bg-p-pg-bg text-p-pg">
                <svg class="h-6 w-6" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><ellipse cx="12" cy="5.5" rx="7.5" ry="2.5" /><path d="M4.5 5.5v13c0 1.4 3.4 2.5 7.5 2.5s7.5-1.1 7.5-2.5v-13" /><path d="M4.5 12c0 1.4 3.4 2.5 7.5 2.5s7.5-1.1 7.5-2.5" /></svg>
              </span>
              <h2 class="mb-1.5 mt-4 text-[17px] font-semibold tracking-tight">Query a local Postgres</h2>
              <p class="text-ink-2">
                <template v-if="!state.view">Collecting…</template>
                <template v-else>LsDev found no running Postgres. Start one, or add a connection, to write SQL and browse the results here.</template>
              </p>
              <div class="mt-[18px] flex gap-2">
                <button type="button" :class="BTN_PRIMARY" @click="openDialog(null)">Add connection…</button>
              </div>
            </div>
          </div>
        </section>

        <ResultGrid v-if="!empty" class="min-h-0 flex-1" :output="output" :error="error" :running="running" />
      </div>
    </div>

    <ConnectionDialog :open="dialogOpen" :initial="dialogInitial" @close="dialogOpen = false" @saved="dialogOpen = false" />
  </div>
</template>
