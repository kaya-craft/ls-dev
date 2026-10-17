import { reactive } from 'vue'
import { nextSort, type SortKey, type SortState } from '../sort'

const STORAGE_KEY = 'lsdev.view'

type ViewState = { sort: SortState; collapsed: string[] }

function parseSort(value: unknown): SortState {
  if (!value || typeof value !== 'object') return null
  const { key, dir } = value as { key?: unknown; dir?: unknown }
  const okKey = key === 'uptime' || key === 'cpu' || key === 'memory'
  const okDir = dir === 'desc' || dir === 'asc'
  return okKey && okDir ? { key, dir } : null
}

function load(): ViewState {
  try {
    const raw = localStorage.getItem(STORAGE_KEY)
    if (!raw) return { sort: null, collapsed: [] }
    const data = JSON.parse(raw) as { sort?: unknown; collapsed?: unknown }
    const collapsed = Array.isArray(data.collapsed)
      ? data.collapsed.filter((x): x is string => typeof x === 'string')
      : []
    return { sort: parseSort(data.sort), collapsed }
  } catch {
    return { sort: null, collapsed: [] }
  }
}

function save() {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify({ sort: state.sort, collapsed: state.collapsed }))
  } catch {
    // Storage unavailable: the view still works for this session.
  }
}

const state = reactive<ViewState>(load())

export function toggleSort(key: SortKey) {
  state.sort = nextSort(state.sort, key)
  save()
}

export function toggleCollapsed(project: string) {
  const i = state.collapsed.indexOf(project)
  if (i >= 0) state.collapsed.splice(i, 1)
  else state.collapsed.push(project)
  save()
}

export function isCollapsed(project: string): boolean {
  return state.collapsed.includes(project)
}

/** For tests: back to defaults, without touching storage. */
export function resetView() {
  state.sort = null
  state.collapsed = []
}

export function useView() {
  return { state }
}
