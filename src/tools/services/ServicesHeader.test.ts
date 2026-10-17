import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import ServicesHeader from './ServicesHeader.vue'
import { applySnapshot, resetServices, useServices } from './store/services'
import { resetNavigation, setServicesSection, toggleSidebar, useNavigation } from '../../shell/navigation'
import { isNarrow } from '../../shell/viewport'
import { snapshot } from '../../test/fixtures'
import { useSettings } from '../../ui/store/settings'
import { api } from './api'
import { runComposeAction } from './actions'
import { coreApi } from '../../core/api'

vi.mock('./api', () => ({
  api: { refresh: vi.fn() },
}))
vi.mock('./actions', () => ({ runComposeAction: vi.fn() }))
vi.mock('../../core/api', () => ({
  coreApi: { setSettings: vi.fn() },
}))

const refreshButton = (wrapper: ReturnType<typeof mount>) => wrapper.find('[aria-label="Refresh"]')

describe('ServicesHeader', () => {
  beforeEach(() => {
    resetServices()
    resetNavigation()
    isNarrow.value = false
    useSettings().settings = null
    vi.mocked(api.refresh).mockReset()
    vi.mocked(coreApi.setSettings).mockReset()
    vi.mocked(runComposeAction).mockReset()
  })
  afterEach(() => {
    isNarrow.value = false
  })

  it('reports a failed refresh in the error banner', async () => {
    vi.mocked(api.refresh).mockRejectedValue('refresh boom')
    const wrapper = mount(ServicesHeader)
    await refreshButton(wrapper).trigger('click')
    await flushPromises()
    expect(useServices().state.error).toBe('refresh boom')
    expect(wrapper.find('[role="alert"]').text()).toContain('refresh boom')
  })

  it('disables Show all until settings are loaded', () => {
    const box = mount(ServicesHeader).find('input[type="checkbox"]')
    expect(box.attributes('disabled')).toBeDefined()
  })

  it('reports a failed settings save and reverts the switch', async () => {
    useSettings().settings = {
      pollIntervalMs: 3000, showAll: false, terminal: 'terminal-app',
      editorCommand: 'code', notifyOnStop: true, launchAtLogin: false, theme: 'system', worktreeRoots: [], worktreeExcludes: [],
      tunnelDomain: '', tunnelPrefix: '', tunnelHours: 4,
    }
    vi.mocked(coreApi.setSettings).mockRejectedValue('save boom')
    const wrapper = mount(ServicesHeader)
    await wrapper.find('input[type="checkbox"]').setValue(true)
    await flushPromises()
    expect(useServices().state.error).toBe('save boom')
    expect((wrapper.find('input[type="checkbox"]').element as HTMLInputElement).checked).toBe(false)
  })

  it('focuses the filter through focusFilter (⌘F)', () => {
    const wrapper = mount(ServicesHeader, { attachTo: document.body })
    ;(wrapper.vm as unknown as { focusFilter: () => void }).focusFilter()
    expect(document.activeElement).toBe(wrapper.find('input[type="search"]').element)
    wrapper.unmount()
  })

  it('emits open-settings from the Settings button', async () => {
    const wrapper = mount(ServicesHeader)
    await wrapper.find('[aria-label="Settings"]').trigger('click')
    expect(wrapper.emitted('open-settings')).toHaveLength(1)
  })

  it('shows the Services eyebrow and the section name as title', async () => {
    applySnapshot(snapshot())
    const wrapper = mount(ServicesHeader)
    expect(wrapper.find('[data-testid="eyebrow"]').text()).toBe('Services')
    expect(wrapper.find('h1').text()).toBe('All listening')
    setServicesSection('dev')
    await wrapper.vm.$nextTick()
    expect(wrapper.find('h1').text()).toBe('Dev servers')
    setServicesSection('standalone')
    await wrapper.vm.$nextTick()
    expect(wrapper.find('h1').text()).toBe('Standalone')
    setServicesSection({ compose: 'acme-shop' })
    await wrapper.vm.$nextTick()
    expect(wrapper.find('h1').text()).toBe('acme-shop')
  })

  it('has no section chip and no title dropdown on a wide window with the sidebar open', () => {
    applySnapshot(snapshot())
    setServicesSection('dev')
    const wrapper = mount(ServicesHeader)
    expect(wrapper.find('[aria-expanded]').exists()).toBe(false)
  })

  it('shows the timestamp as text on a wide window, not in the Refresh title', () => {
    applySnapshot(snapshot())
    const wrapper = mount(ServicesHeader)
    expect(wrapper.find('[data-testid="updated"]').text()).toMatch(/^Updated \d{2}:\d{2}:\d{2}$/)
    expect(refreshButton(wrapper).attributes('title')).toBe('Refresh (⌘R)')
  })

  it('shows the compose working dir and runs Start all / Stop all on the group', async () => {
    applySnapshot(snapshot())
    setServicesSection({ compose: 'acme-shop' })
    const wrapper = mount(ServicesHeader)
    expect(wrapper.find('[data-testid="subtitle"]').text()).toBe('Compose · /w/shop/infra')
    const group = useServices().state.snapshot!.compose[0]
    const buttons = wrapper.findAll('button')
    await buttons.find((b) => b.text() === 'Start all')!.trigger('click')
    expect(runComposeAction).toHaveBeenLastCalledWith(group, 'start')
    await buttons.find((b) => b.text() === 'Stop all')!.trigger('click')
    expect(runComposeAction).toHaveBeenLastCalledWith(group, 'stop')
  })

  it('shows no subtitle and no Start all / Stop all outside a compose section', () => {
    applySnapshot(snapshot())
    const wrapper = mount(ServicesHeader)
    expect(wrapper.find('[data-testid="subtitle"]').exists()).toBe(false)
    expect(wrapper.text()).not.toContain('Start all')
  })

  describe('title dropdown', () => {
    async function open(wrapper: ReturnType<typeof mount>) {
      await wrapper.find('[aria-expanded]').trigger('click')
      return wrapper.findAll('[data-testid="section-option"]').map((i) => i.text())
    }

    it('narrow: lists every section and switches on choice', async () => {
      applySnapshot(snapshot())
      isNarrow.value = true
      const wrapper = mount(ServicesHeader)
      expect(await open(wrapper)).toEqual(['All listening', 'Dev servers', 'acme-shop', 'Standalone'])
      await wrapper.findAll('[data-testid="section-option"]')[2].trigger('click')
      expect(useNavigation().state.servicesSection).toEqual({ compose: 'acme-shop' })
      expect(wrapper.find('[data-testid="section-list"]').exists()).toBe(false)
    })

    it('closes on outside click', async () => {
      applySnapshot(snapshot())
      isNarrow.value = true
      const wrapper = mount(ServicesHeader, { attachTo: document.body })
      await open(wrapper)
      expect(wrapper.find('[data-testid="section-list"]').exists()).toBe(true)
      document.body.click()
      await wrapper.vm.$nextTick()
      expect(wrapper.find('[data-testid="section-list"]').exists()).toBe(false)
      wrapper.unmount()
    })

    it('closes on Esc without reaching the global Esc handler', async () => {
      applySnapshot(snapshot())
      isNarrow.value = true
      const wrapper = mount(ServicesHeader, { attachTo: document.body })
      await open(wrapper)
      const reached = vi.fn()
      window.addEventListener('keydown', reached)
      document.body.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }))
      await wrapper.vm.$nextTick()
      expect(wrapper.find('[data-testid="section-list"]').exists()).toBe(false)
      expect(reached).not.toHaveBeenCalled()
      // Menu closed: Esc passes through again.
      document.body.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }))
      expect(reached).toHaveBeenCalledTimes(1)
      window.removeEventListener('keydown', reached)
      wrapper.unmount()
    })

    it('closes when the sidebar sub-list comes back', async () => {
      applySnapshot(snapshot())
      isNarrow.value = true
      const wrapper = mount(ServicesHeader)
      await open(wrapper)
      isNarrow.value = false
      await wrapper.vm.$nextTick()
      isNarrow.value = true
      await wrapper.vm.$nextTick()
      expect(wrapper.find('[data-testid="section-list"]').exists()).toBe(false)
    })

    it('closes when focus leaves the dropdown', async () => {
      applySnapshot(snapshot())
      isNarrow.value = true
      const wrapper = mount(ServicesHeader, { attachTo: document.body })
      await open(wrapper)
      expect(wrapper.find('[data-testid="section-list"]').exists()).toBe(true)
      await wrapper.find('[data-testid="section-list"]').trigger('focusout', { relatedTarget: document.body })
      expect(wrapper.find('[data-testid="section-list"]').exists()).toBe(false)
      wrapper.unmount()
    })

    it('uses the disclosure pattern, without menu roles', async () => {
      applySnapshot(snapshot())
      isNarrow.value = true
      const wrapper = mount(ServicesHeader)
      await open(wrapper)
      expect(wrapper.find('[role="menu"]').exists()).toBe(false)
      expect(wrapper.find('[role="menuitem"]').exists()).toBe(false)
      expect(wrapper.find('[aria-expanded]').attributes('aria-expanded')).toBe('true')
    })

    it('keeps the h1 outside the button', async () => {
      applySnapshot(snapshot())
      isNarrow.value = true
      const wrapper = mount(ServicesHeader)
      expect(wrapper.find('button h1').exists()).toBe(false)
      expect(wrapper.find('h1 [aria-expanded]').text()).toBe('All listening')
    })

    it('wide with the sidebar collapsed by the user: same dropdown', async () => {
      applySnapshot(snapshot())
      toggleSidebar()
      const wrapper = mount(ServicesHeader)
      expect(await open(wrapper)).toEqual(['All listening', 'Dev servers', 'acme-shop', 'Standalone'])
      await wrapper.findAll('[data-testid="section-option"]')[1].trigger('click')
      expect(useNavigation().state.servicesSection).toBe('dev')
    })
  })

  it('narrow: no eyebrow, 15 px title; wide: 60 px header with eyebrow and draggable timestamp', () => {
    applySnapshot(snapshot())
    isNarrow.value = true
    const narrow = mount(ServicesHeader)
    expect(narrow.find('[data-testid="eyebrow"]').exists()).toBe(false)
    expect(narrow.find('h1').classes()).toContain('text-[15px]')
    isNarrow.value = false
    const wide = mount(ServicesHeader)
    expect(wide.find('[data-testid="eyebrow"]').exists()).toBe(true)
    expect(wide.find('header').classes()).toContain('h-[60px]')
    expect(wide.find('[data-testid="updated"]').attributes()).toHaveProperty('data-tauri-drag-region')
  })

  it('narrow: short placeholder, All label, timestamp only in the Refresh title', () => {
    applySnapshot(snapshot())
    isNarrow.value = true
    const wrapper = mount(ServicesHeader)
    expect(wrapper.find('input[type="search"]').attributes('placeholder')).toBe('Filter…')
    expect(wrapper.find('input[type="checkbox"]').element.parentElement!.textContent!.trim()).toBe('All')
    expect(wrapper.text()).not.toContain('Show all')
    expect(wrapper.find('[data-testid="updated"]').exists()).toBe(false)
    expect(refreshButton(wrapper).attributes('title')).toMatch(/^Refresh \(⌘R\) · Updated \d{2}:\d{2}:\d{2}$/)
  })

  it('wide: full placeholder and Show all label', () => {
    const wrapper = mount(ServicesHeader)
    expect(wrapper.find('input[type="search"]').attributes('placeholder')).toBe('Filter by name, command, image or port')
    expect(wrapper.text()).toContain('Show all')
  })
})
