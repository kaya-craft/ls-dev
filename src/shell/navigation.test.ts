import { beforeEach, describe, expect, it } from 'vitest'

const KEY = 'lsdev.navigation'

async function fresh() {
  const { vi } = await import('vitest')
  vi.resetModules()
  return import('./navigation')
}

describe('navigation store', () => {
  beforeEach(() => localStorage.clear())

  it('defaults to Services, expanded sidebar, all sections', async () => {
    const { useNavigation } = await fresh()
    expect(useNavigation().state).toEqual({ currentTool: 'services', sidebarCollapsed: false, servicesSection: 'all' })
  })

  it('persists tool, sidebar and section', async () => {
    const nav = await fresh()
    nav.setTool('worktrees')
    nav.toggleSidebar()
    nav.setServicesSection({ compose: 'acme' })
    const again = await fresh()
    expect(again.useNavigation().state).toEqual({ currentTool: 'worktrees', sidebarCollapsed: true, servicesSection: { compose: 'acme' } })
  })

  it('lists Tunnels at ⌘4 and no Domains', async () => {
    const { TOOLS } = await fresh()
    expect(TOOLS[3]).toEqual({ id: 'tunnels', label: 'Tunnels', shortcut: '⌘4' })
    expect(TOOLS.some((t) => (t.id as string) === 'domains')).toBe(false)
  })

  it('falls back to Services when the stored tool is the removed Domains page', async () => {
    localStorage.setItem(KEY, JSON.stringify({ currentTool: 'domains', sidebarCollapsed: true, servicesSection: 'dev' }))
    const { useNavigation } = await fresh()
    expect(useNavigation().state).toEqual({ currentTool: 'services', sidebarCollapsed: true, servicesSection: 'dev' })
  })

  it('falls back to defaults on corrupted or unknown data', async () => {
    localStorage.setItem(KEY, '{"currentTool":"spaceship","servicesSection":42')
    const { useNavigation } = await fresh()
    expect(useNavigation().state.currentTool).toBe('services')
    localStorage.setItem(KEY, JSON.stringify({ currentTool: 'spaceship', sidebarCollapsed: 'yes', servicesSection: 42 }))
    const again = await fresh()
    expect(again.useNavigation().state).toEqual({ currentTool: 'services', sidebarCollapsed: false, servicesSection: 'all' })
  })
})
