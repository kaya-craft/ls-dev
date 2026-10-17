import { reactive } from 'vue'
import { api, type LogChunk, type LogLine } from '../api'
import { isProcess, type Service } from '../types'

export const MAX_LINES = 5000

type Line = LogLine & { system?: boolean }

const state = reactive({
  id: null as string | null,
  lines: [] as Line[],
  pending: [] as Line[],
  paused: false,
  unavailable: false,
  error: null as string | null,
  filter: '',
  /** Increments on every append, so views can follow output once the buffer is capped. */
  version: 0,
})

function append(lines: Line[]) {
  state.version++
  state.lines.push(...lines)
  if (state.lines.length > MAX_LINES) state.lines.splice(0, state.lines.length - MAX_LINES)
}

export async function openLogs(service: Service) {
  resetLogs()
  state.id = service.id
  if (isProcess(service) && !service.managed) {
    state.unavailable = true
    return
  }
  try {
    await api.logsOpen(service.id)
  } catch (error) {
    state.error = String(error)
  }
}

export function closeLogs() {
  if (state.id !== null) void api.logsClose()
  resetLogs()
}

export function handleChunk(chunk: LogChunk) {
  if (chunk.id !== state.id) return
  if (state.paused) state.pending.push(...chunk.lines)
  else append(chunk.lines)
}

export function handleClosed(event: { id: string; reason: string }) {
  if (event.id !== state.id) return
  append([{ stream: 'stdout', line: `— stream closed: ${event.reason} —`, system: true }])
}

export function setPaused(paused: boolean) {
  state.paused = paused
  if (!paused && state.pending.length > 0) {
    append(state.pending)
    state.pending = []
  }
}

export function clearLogs() {
  state.lines = []
  state.pending = []
}

export function resetLogs() {
  state.id = null
  state.lines = []
  state.pending = []
  state.paused = false
  state.unavailable = false
  state.error = null
  state.filter = ''
}

export function useLogs() {
  return state
}
