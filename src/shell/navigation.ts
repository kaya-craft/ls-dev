import { reactive } from 'vue'

export type Tool = 'services' | 'worktrees' | 'databases' | 'tunnels'
export type ServicesSection = 'all' | 'dev' | 'standalone' | { compose: string }

export const TOOLS: { id: Tool; label: string; shortcut: string }[] = [
  { id: 'services', label: 'Services', shortcut: '⌘1' },
  { id: 'worktrees', label: 'Worktrees', shortcut: '⌘2' },
  { id: 'databases', label: 'Databases', shortcut: '⌘3' },
  { id: 'tunnels', label: 'Tunnels', shortcut: '⌘4' },
]

const KEY = 'lsdev.navigation'
type NavState = { currentTool: Tool; sidebarCollapsed: boolean; servicesSection: ServicesSection }
const DEFAULTS: NavState = { currentTool: 'services', sidebarCollapsed: false, servicesSection: 'all' }

function parseSection(v: unknown): ServicesSection {
  if (v === 'all' || v === 'dev' || v === 'standalone') return v
  if (v && typeof v === 'object' && typeof (v as { compose?: unknown }).compose === 'string') {
    return { compose: (v as { compose: string }).compose }
  }
  return 'all'
}

function load(): NavState {
  try {
    const raw = localStorage.getItem(KEY)
    if (!raw) return { ...DEFAULTS }
    const d = JSON.parse(raw) as Record<string, unknown>
    return {
      currentTool: TOOLS.some((t) => t.id === d.currentTool) ? (d.currentTool as Tool) : 'services',
      sidebarCollapsed: d.sidebarCollapsed === true,
      servicesSection: parseSection(d.servicesSection),
    }
  } catch {
    return { ...DEFAULTS }
  }
}

const state = reactive<NavState>(load())

function save() {
  try {
    localStorage.setItem(KEY, JSON.stringify(state))
  } catch {
    // Storage unavailable: navigation still works for this session.
  }
}

export function setTool(tool: Tool) {
  state.currentTool = tool
  save()
}

export function toggleSidebar() {
  state.sidebarCollapsed = !state.sidebarCollapsed
  save()
}

export function setServicesSection(section: ServicesSection) {
  state.servicesSection = section
  save()
}

/** For tests: back to defaults, without touching storage. */
export function resetNavigation() {
  Object.assign(state, DEFAULTS)
}

export function useNavigation() {
  return { state }
}
