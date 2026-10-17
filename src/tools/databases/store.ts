import { computed, reactive } from 'vue'
import { databasesApi } from './api'
import type { ConnectionsView, RunError, RunOutput } from './types'

const DRAFTS_KEY = 'lsdev.databases.drafts'
const ACTIVE_KEY = 'lsdev.databases.active'

function readStorage(key: string): string | null {
  try {
    return localStorage.getItem(key)
  } catch {
    return null
  }
}

function writeStorage(key: string, value: string | null) {
  try {
    if (value === null) localStorage.removeItem(key)
    else localStorage.setItem(key, value)
  } catch {
    // Persistence is a convenience; the in-memory state stays authoritative.
  }
}

function readDrafts(): Record<string, string> {
  try {
    const parsed: unknown = JSON.parse(readStorage(DRAFTS_KEY) ?? '{}')
    if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) return {}
    return Object.fromEntries(Object.entries(parsed).filter(([, v]) => typeof v === 'string')) as Record<string, string>
  } catch {
    return {}
  }
}

const state = reactive({
  view: null as ConnectionsView | null,
  activeId: readStorage(ACTIVE_KEY) as string | null,
  drafts: readDrafts(),
  /** Run state is keyed by connection id: a run finishing after a switch stays with its connection. */
  outputs: {} as Record<string, RunOutput>,
  errors: {} as Record<string, RunError>,
  running: {} as Record<string, boolean>,
  /** Read-only of detected connections; the backend keeps it in memory and does not report it. */
  detectedReadOnly: {} as Record<string, boolean>,
})

/** Clears the in-memory state and reloads `drafts` and `activeId` from localStorage. */
export function resetDatabases() {
  state.view = null
  state.activeId = readStorage(ACTIVE_KEY)
  state.drafts = readDrafts()
  state.outputs = {}
  state.errors = {}
  state.running = {}
  state.detectedReadOnly = {}
}

function toRunError(e: unknown): RunError {
  if (e && typeof e === 'object' && typeof (e as { kind?: unknown }).kind === 'string') return e as RunError
  const message = e instanceof Error ? e.message : String(e)
  return { kind: 'other', message }
}

/** Dot colour of a connection: its own colour, else a default by env. */
export function connectionColor(c: { color: string | null; env: string | null }): string {
  if (c.color) return c.color
  if (c.env === 'production') return 'var(--color-danger)'
  if (c.env === 'staging') return 'var(--color-warm)'
  return 'var(--color-accent)'
}

/** Display text of a run error. The `connect` text mirrors `connect_message` in session.rs. */
export function errorText(error: RunError): string {
  switch (error.kind) {
    case 'connect': {
      let text = `Could not connect to ${error.host}:${error.port}. ${error.cause}`
      if (['127.0.0.1', 'localhost', '::1'].includes(error.host) && error.port !== 5432) text += ' Is the tunnel running?'
      return text
    }
    case 'closed':
      return 'The connection was closed. Run the query again.'
    case 'db': {
      if (error.code === '25006') return 'Read-only is on for this connection. Turn it off to write.'
      return [error.message, `SQLSTATE ${error.code}`, error.hint].filter((l): l is string => !!l).join('\n')
    }
    case 'cancelled':
      return 'Query cancelled.'
    case 'other':
      return error.message
  }
}

const active = computed(() => {
  const id = state.activeId
  const view = state.view
  if (!id || !view) return null
  const saved = view.saved.find((c) => c.id === id)
  if (saved) return { id, name: saved.name, readOnly: saved.readOnly, color: saved.color, saved }
  const detected = view.detected.find((c) => c.id === id)
  if (detected) return { id, name: detected.name, readOnly: state.detectedReadOnly[id] ?? false, color: null as string | null, detected }
  return null
})

const output = computed(() => (state.activeId ? (state.outputs[state.activeId] ?? null) : null))
const error = computed(() => (state.activeId ? (state.errors[state.activeId] ?? null) : null))
const running = computed(() => (state.activeId ? (state.running[state.activeId] ?? false) : false))

const draft = computed(() => (state.activeId ? (state.drafts[state.activeId] ?? '') : ''))

async function refresh() {
  state.view = await databasesApi.list()
  const id = state.activeId
  if (id && !state.view.saved.some((c) => c.id === id) && !state.view.detected.some((c) => c.id === id)) {
    state.activeId = null
    writeStorage(ACTIVE_KEY, null)
  }
}

function select(id: string) {
  state.activeId = id
  writeStorage(ACTIVE_KEY, id)
}

function setDraft(id: string, sql: string) {
  state.drafts[id] = sql
  writeStorage(DRAFTS_KEY, JSON.stringify(state.drafts))
}

async function run() {
  const id = state.activeId
  if (!id || state.running[id]) return
  state.running[id] = true
  delete state.outputs[id]
  delete state.errors[id]
  try {
    state.outputs[id] = await databasesApi.run(id, state.drafts[id] ?? '')
  } catch (e) {
    state.errors[id] = toRunError(e)
  } finally {
    state.running[id] = false
  }
}

/** A failed cancel is stored as an error of that connection. */
async function cancel() {
  const id = state.activeId
  if (!id) return
  try {
    await databasesApi.cancel(id)
  } catch (e) {
    state.errors[id] = toRunError(e)
  }
}

/** Rethrows a backend failure without changing state; the caller shows it. */
async function setReadOnly(id: string, on: boolean) {
  await databasesApi.setReadOnly(id, on)
  if (state.view?.detected.some((c) => c.id === id)) state.detectedReadOnly[id] = on
  else await refresh()
}

export function useDatabases() {
  return { state, active, draft, output, error, running, refresh, select, setDraft, run, cancel, setReadOnly }
}
