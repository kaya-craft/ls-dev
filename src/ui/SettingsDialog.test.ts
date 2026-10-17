import { beforeEach, describe, expect, it, vi } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'
import SettingsDialog from './SettingsDialog.vue'
import { coreApi } from '../core/api'
import { useSettings } from './store/settings'
import { tunnelsApi } from '../tools/tunnels/api'
import type { Settings } from '../core/types'

vi.mock('../core/api', () => ({ coreApi: { setSettings: vi.fn(async (s: Settings) => s) } }))
vi.mock('../tools/tunnels/api', () => ({ tunnelsApi: { status: vi.fn(async () => null) } }))

const base: Settings = {
  pollIntervalMs: 3000, showAll: false, terminal: 'terminal-app',
  editorCommand: 'cursor', notifyOnStop: true, launchAtLogin: false, theme: 'system', worktreeRoots: [], worktreeExcludes: [],
  tunnelDomain: '', tunnelPrefix: '', tunnelHours: 4,
}

describe('SettingsDialog', () => {
  beforeEach(() => { useSettings().settings = { ...base }; vi.clearAllMocks() })

  it('saves the interval in milliseconds', async () => {
    const w = mount(SettingsDialog, { props: { open: true } })
    await w.find('input[name="interval"]').setValue('5')
    await w.find('form').trigger('submit')
    await flushPromises()
    expect(coreApi.setSettings).toHaveBeenCalledWith(expect.objectContaining({ pollIntervalMs: 5000 }))
    expect(w.emitted('close')).toBeTruthy()
  })

  it('saves the tunnel domain, prefix and default duration', async () => {
    useSettings().settings = { ...base, tunnelHours: 8 }
    const w = mount(SettingsDialog, { props: { open: true } })
    expect(w.find('input[name="tunnelDomain"]').attributes('placeholder')).toBe('dev.example.com')
    expect(w.find('input[name="tunnelPrefix"]').attributes('placeholder')).toBe('your macOS user name')
    expect((w.find('select[name="tunnelHours"]').element as HTMLSelectElement).value).toBe('8')
    await w.find('input[name="tunnelDomain"]').setValue('dev.acme.io')
    await w.find('input[name="tunnelPrefix"]').setValue('kl')
    await w.find('select[name="tunnelHours"]').setValue('24')
    await w.find('form').trigger('submit')
    await flushPromises()
    expect(coreApi.setSettings).toHaveBeenCalledWith(
      expect.objectContaining({ tunnelDomain: 'dev.acme.io', tunnelPrefix: 'kl', tunnelHours: 24 }),
    )
  })

  it('shows the stored theme and saves the chosen one', async () => {
    useSettings().settings = { ...base, theme: 'dark' }
    const w = mount(SettingsDialog, { props: { open: true } })
    expect((w.find('select[name="theme"]').element as HTMLSelectElement).value).toBe('dark')
    await w.find('select[name="theme"]').setValue('light')
    await w.find('form').trigger('submit')
    await flushPromises()
    expect(coreApi.setSettings).toHaveBeenCalledWith(expect.objectContaining({ theme: 'light' }))
  })

  it('saves cmux as a terminal and explains its socket setting', async () => {
    const w = mount(SettingsDialog, { props: { open: true } })
    expect(w.find('[data-testid="cmux-hint"]').exists()).toBe(false)
    await w.find('select[name="terminal"]').setValue('cmux')
    expect(w.find('[data-testid="cmux-hint"]').text()).toContain('Automation')
    await w.find('form').trigger('submit')
    await flushPromises()
    expect(coreApi.setSettings).toHaveBeenCalledWith(expect.objectContaining({ terminal: 'cmux' }))
  })

  it('requires {cmd} in a custom terminal command', async () => {
    const w = mount(SettingsDialog, { props: { open: true } })
    await w.find('select[name="terminal"]').setValue('custom')
    await w.find('input[name="custom"]').setValue('ghostty -e')
    await w.find('form').trigger('submit')
    expect(coreApi.setSettings).not.toHaveBeenCalled()
    expect(w.text()).toContain('must contain {cmd}')
  })

  it('stores a custom terminal as an object', async () => {
    const w = mount(SettingsDialog, { props: { open: true } })
    await w.find('select[name="terminal"]').setValue('custom')
    await w.find('input[name="custom"]').setValue('cmux run {cmd}')
    await w.find('form').trigger('submit')
    await flushPromises()
    expect(coreApi.setSettings).toHaveBeenCalledWith(expect.objectContaining({ terminal: { custom: 'cmux run {cmd}' } }))
  })

  it('stays open with an error when settings were never loaded', async () => {
    const w = mount(SettingsDialog, { props: { open: true } })
    useSettings().settings = null
    await w.find('form').trigger('submit')
    await flushPromises()
    expect(coreApi.setSettings).not.toHaveBeenCalled()
    expect(w.emitted('close')).toBeFalsy()
    expect(w.find('[role="alert"]').text()).toBe('Settings are not loaded yet.')
  })

  it('refreshes the tunnels status after a successful save only', async () => {
    const w = mount(SettingsDialog, { props: { open: true } })
    await w.find('input[name="tunnelDomain"]').setValue('dev.acme.io')
    await w.find('form').trigger('submit')
    await flushPromises()
    expect(tunnelsApi.status).toHaveBeenCalledTimes(1)
    vi.mocked(coreApi.setSettings).mockRejectedValueOnce('disk full')
    await w.find('form').trigger('submit')
    await flushPromises()
    expect(tunnelsApi.status).toHaveBeenCalledTimes(1)
  })
})
