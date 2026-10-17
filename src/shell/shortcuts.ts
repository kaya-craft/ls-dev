import { TOOLS, type Tool } from './navigation'

export type ShortcutHandlers = {
  refresh: () => void
  focusFilter: () => void
  openSettings: () => void
  escape: () => void
  switchTool: (tool: Tool) => void
  toggleSidebar: () => void
}

export type ShortcutContext = { tool: Tool; modalOpen: boolean; narrow: boolean }

/** Tool index (0-3) for ⌘1–⌘4: physical digit key first, `event.key` as fallback. */
function toolIndex(event: KeyboardEvent): number | null {
  const m = /^(?:Digit|Numpad)([1-4])$/.exec(event.code ?? '')
  if (m) return Number(m[1]) - 1
  return /^[1-4]$/.test(event.key) ? Number(event.key) - 1 : null
}

function isBackslash(event: KeyboardEvent): boolean {
  return event.code === 'Backslash' || event.code === 'IntlBackslash' || event.key === '\\'
}

/** Returns true when the event was handled. ⌘R must not reload the webview. */
export function handleShortcut(event: KeyboardEvent, h: ShortcutHandlers, ctx: ShortcutContext): boolean {
  const key = event.key.toLowerCase()
  let action: (() => void) | null = null
  if (event.key === 'Escape') action = h.escape
  else if (event.metaKey && event.key === ',') action = h.openSettings
  else if (event.metaKey && key === 'r' && ctx.tool === 'services') action = h.refresh
  else if (event.metaKey && key === 'f' && ctx.tool === 'services') action = h.focusFilter
  else if (event.metaKey && isBackslash(event) && !ctx.modalOpen && !ctx.narrow) action = h.toggleSidebar
  else if (event.metaKey && !ctx.modalOpen && toolIndex(event) !== null) {
    const tool = TOOLS[toolIndex(event)!].id
    action = () => h.switchTool(tool)
  }
  if (!action) return false
  event.preventDefault()
  action()
  return true
}
