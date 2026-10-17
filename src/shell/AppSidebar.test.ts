import { beforeEach, describe, expect, it } from 'vitest'
import { mount } from '@vue/test-utils'
import AppSidebar from './AppSidebar.vue'
import { resetNavigation, useNavigation } from './navigation'
import { applySnapshot, resetServices } from '../tools/services/store/services'
import { snapshot } from '../test/fixtures'
import { applyWorktrees, resetWorktrees } from '../tools/worktrees/store'

describe('AppSidebar', () => {
  beforeEach(() => { resetNavigation(); resetServices(); applySnapshot(snapshot()) })

  it('lists the four tools and switches tool on click', async () => {
    const w = mount(AppSidebar)
    const labels = w.findAll('[data-testid="tool"]').map((b) => b.attributes('aria-label') ?? b.text())
    expect(labels.join(' ')).toMatch(/Services.*Worktrees.*Databases.*Tunnels/)
    await w.findAll('[data-testid="tool"]')[1].trigger('click')
    expect(useNavigation().state.currentTool).toBe('worktrees')
  })

  it('shows Services sections only when Services is current and the sidebar is expanded', async () => {
    const w = mount(AppSidebar)
    expect(w.text()).toContain('All listening')
    expect(w.text()).toContain('acme-shop')
    useNavigation().state.currentTool = 'worktrees'
    await w.vm.$nextTick()
    expect(w.text()).not.toContain('All listening')
  })

  it('collapses to an icon rail with tooltips', async () => {
    const w = mount(AppSidebar)
    await w.find('[aria-label="Collapse sidebar"]').trigger('click')
    expect(useNavigation().state.sidebarCollapsed).toBe(true)
    expect(w.find('[aria-label="Expand sidebar"]').exists()).toBe(true)
    expect(w.findAll('[data-testid="tool"]')[2].attributes('title')).toBe('Databases ⌘3')
  })

  it('shows Docker status in the footer', () => {
    expect(mount(AppSidebar).text()).toContain('Docker running')
  })

  it('hides the collapse and expand toggles while the window is narrow', async () => {
    const original = window.innerWidth
    Object.defineProperty(window, 'innerWidth', { configurable: true, value: 800 })
    window.dispatchEvent(new Event('resize'))
    try {
      const w = mount(AppSidebar)
      expect(w.find('[aria-label="Collapse sidebar"]').exists()).toBe(false)
      expect(w.find('[aria-label="Expand sidebar"]').exists()).toBe(false)
      expect(useNavigation().state.sidebarCollapsed).toBe(false)
    } finally {
      Object.defineProperty(window, 'innerWidth', { configurable: true, value: original })
      window.dispatchEvent(new Event('resize'))
    }
  })

  it('highlights All listening when the stored compose section is not in the snapshot', async () => {
    useNavigation().state.servicesSection = { compose: 'gone' }
    const w = mount(AppSidebar)
    const subs = w.findAll('button').filter((b) => b.text().startsWith('All listening'))
    expect(subs[0].classes()).toContain('bg-active')
    expect(useNavigation().state.servicesSection).toEqual({ compose: 'gone' })
  })

  it('reserves a 44 px drag strip for the traffic lights, expanded and collapsed', async () => {
    const w = mount(AppSidebar)
    expect(w.find('[data-tauri-drag-region]').classes()).toContain('h-11')
    await w.find('[aria-label="Collapse sidebar"]').trigger('click')
    expect(w.find('[data-tauri-drag-region]').classes()).toContain('h-11')
    expect(w.find('[aria-label="Collapse sidebar"]').exists()).toBe(false)
  })
})

describe('AppSidebar Worktrees count', () => {
  beforeEach(() => { resetNavigation(); resetServices(); applySnapshot(snapshot()); resetWorktrees() })

  const info = (path: string) => ({
    path, branch: 'b', lastCommitAt: 1, changes: 0, upstream: { kind: 'none' as const }, merged: false, trusted: true, sizeBytes: 1,
  })

  it('shows the number of linked worktrees next to the Worktrees entry, and nothing before the first scan', async () => {
    const w = mount(AppSidebar)
    const entry = () => w.findAll('[data-testid="tool"]')[1]
    expect(entry().text()).toContain('⌘2')
    applyWorktrees({
      takenAt: 1,
      scanning: false,
      repos: [
        { path: '/r/a', name: 'a', mainSize: null, worktrees: [info('/w/1'), info('/w/2')] },
        { path: '/r/b', name: 'b', mainSize: null, worktrees: [info('/w/3')] },
      ],
    })
    await w.vm.$nextTick()
    expect(entry().text()).toContain('3')
    expect(entry().text()).not.toContain('⌘2')
  })
})
