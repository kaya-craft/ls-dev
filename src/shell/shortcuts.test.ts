import { describe, expect, it, vi } from 'vitest'
import { handleShortcut } from './shortcuts'

function key(k: string, meta = false) {
  return new KeyboardEvent('keydown', { key: k, metaKey: meta, cancelable: true })
}

describe('handleShortcut', () => {
  const handlers = () => ({ refresh: vi.fn(), focusFilter: vi.fn(), openSettings: vi.fn(), escape: vi.fn(), switchTool: vi.fn(), toggleSidebar: vi.fn() })
  const ctx = { tool: 'services' as const, modalOpen: false, narrow: false }

  it('maps ⌘R, ⌘F, ⌘, and Esc and prevents the webview default', () => {
    const h = handlers()
    const r = key('r', true)
    expect(handleShortcut(r, h, ctx)).toBe(true)
    expect(r.defaultPrevented).toBe(true)
    expect(h.refresh).toHaveBeenCalled()
    handleShortcut(key('f', true), h, ctx)
    expect(h.focusFilter).toHaveBeenCalled()
    handleShortcut(key(',', true), h, ctx)
    expect(h.openSettings).toHaveBeenCalled()
    handleShortcut(key('Escape'), h, ctx)
    expect(h.escape).toHaveBeenCalled()
  })

  it('ignores plain letters', () => {
    const h = handlers()
    expect(handleShortcut(key('r'), h, ctx)).toBe(false)
    expect(h.refresh).not.toHaveBeenCalled()
  })
})

describe('tool shortcuts', () => {
  const h = () => ({ refresh: vi.fn(), focusFilter: vi.fn(), openSettings: vi.fn(), escape: vi.fn(), switchTool: vi.fn(), toggleSidebar: vi.fn() })
  const key = (k: string, meta = true) => new KeyboardEvent('keydown', { key: k, metaKey: meta, cancelable: true })

  it('⌘1–⌘4 switch tools and ⌘\\ toggles the sidebar', () => {
    const handlers = h()
    handleShortcut(key('3'), handlers, { tool: 'services', modalOpen: false, narrow: false })
    expect(handlers.switchTool).toHaveBeenCalledWith('databases')
    handleShortcut(key('\\'), handlers, { tool: 'services', modalOpen: false, narrow: false })
    expect(handlers.toggleSidebar).toHaveBeenCalled()
  })

  it('matches the physical key on non-QWERTY layouts (AZERTY digit row)', () => {
    const handlers = h()
    const ev = (k: string, code: string) => new KeyboardEvent('keydown', { key: k, code, metaKey: true, cancelable: true })
    expect(handleShortcut(ev('&', 'Digit1'), handlers, { tool: 'databases', modalOpen: false, narrow: false })).toBe(true)
    expect(handlers.switchTool).toHaveBeenLastCalledWith('services')
    handleShortcut(ev('"', 'Digit3'), handlers, { tool: 'services', modalOpen: false, narrow: false })
    expect(handlers.switchTool).toHaveBeenLastCalledWith('databases')
    handleShortcut(ev('4', 'Numpad4'), handlers, { tool: 'services', modalOpen: false, narrow: false })
    expect(handlers.switchTool).toHaveBeenLastCalledWith('tunnels')
    handleShortcut(ev('/', 'Backslash'), handlers, { tool: 'services', modalOpen: false, narrow: false })
    expect(handlers.toggleSidebar).toHaveBeenCalledTimes(1)
    handleShortcut(ev('<', 'IntlBackslash'), handlers, { tool: 'services', modalOpen: false, narrow: false })
    expect(handlers.toggleSidebar).toHaveBeenCalledTimes(2)
  })

  it('⌘R and ⌘F only act on Services', () => {
    const handlers = h()
    expect(handleShortcut(key('r'), handlers, { tool: 'worktrees', modalOpen: false, narrow: false })).toBe(false)
    expect(handleShortcut(key('f'), handlers, { tool: 'worktrees', modalOpen: false, narrow: false })).toBe(false)
    expect(handlers.refresh).not.toHaveBeenCalled()
  })

  it('⌘\\ is ignored while the window is narrow, tool shortcuts still work', () => {
    const handlers = h()
    expect(handleShortcut(key('\\'), handlers, { tool: 'services', modalOpen: false, narrow: true })).toBe(false)
    expect(handlers.toggleSidebar).not.toHaveBeenCalled()
    expect(handleShortcut(key('2'), handlers, { tool: 'services', modalOpen: false, narrow: true })).toBe(true)
  })

  it('tool switching is ignored while a modal is open', () => {
    const handlers = h()
    expect(handleShortcut(key('2'), handlers, { tool: 'services', modalOpen: true, narrow: false })).toBe(false)
  })
})
