import { flushPromises, mount } from '@vue/test-utils'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { openUrl } from '@tauri-apps/plugin-opener'
import { tunnelsApi } from './api'
import { applyTunnels, resetTunnels, useTunnels } from './store'
import TunnelsPage from './TunnelsPage.vue'
import type { Tunnel, TunnelsStatus } from './types'
import { answer, useConfirm } from '../../ui/store/confirm'
import { useToasts } from '../../ui/store/toasts'
import { applySnapshot, resetServices } from '../services/store/services'
import { ctr, proc, snapshot } from '../../test/fixtures'

vi.mock('@tauri-apps/plugin-opener', () => ({ openUrl: vi.fn() }))
vi.mock('./api', () => ({
  tunnelsApi: { status: vi.fn(), list: vi.fn(), extend: vi.fn(), stop: vi.fn(), output: vi.fn(), deleteStable: vi.fn(), login: vi.fn() },
}))
const api = vi.mocked(tunnelsApi)

const NOW = 1_800_000_000_000
const ready: TunnelsStatus = { binary: '/opt/homebrew/bin/cloudflared', loggedIn: true, domain: 'dev.acme.io', stable: [] }
const tun = (o: Partial<Tunnel> = {}): Tunnel => ({
  id: 't1', kind: 'quick', port: 3000, project: 'acme', cwd: '/x/acme', url: 'https://abc.trycloudflare.com',
  status: 'open', error: null, startedAt: NOW, expiresAt: NOW + (3 * 60 + 12) * 60_000 + 5_000, ...o,
})
const messages = () => useToasts().items.map((t) => `${t.kind}:${t.message}`)
const writeText = vi.fn()

const mounted: Array<{ unmount: () => void }> = []
function page() {
  const w = mount(TunnelsPage, { attachTo: document.body })
  mounted.push(w)
  return w
}
const button = (w: ReturnType<typeof page>, label: string) => w.findAll('button').find((b) => b.text() === label)!

beforeEach(() => {
  vi.useFakeTimers({ toFake: ['setInterval', 'clearInterval', 'Date'] })
  vi.setSystemTime(NOW)
  vi.resetAllMocks()
  resetTunnels()
  resetServices()
  useToasts().items.splice(0)
  useTunnels().state.status = ready
  const s = snapshot()
  s.processes = [proc({ ports: [{ port: 3000, responsive: true }] })]
  s.standalone = [ctr({ ports: [{ port: 5432, responsive: true }] })]
  s.compose = []
  applySnapshot(s)
  Object.assign(navigator, { clipboard: { writeText } })
  writeText.mockResolvedValue(undefined)
  document.body.innerHTML = ''
})
afterEach(() => {
  mounted.splice(0).forEach((w) => w.unmount())
  vi.useRealTimers()
})

describe('TunnelsPage', () => {
  it('shows the header with the Share a port action', () => {
    const w = page()
    expect(w.text()).toContain('Public URLs for local servers')
    expect(w.find('[data-testid="header-share"]').text()).toBe('Share a port')
  })

  it('shows the empty state and opens the share dialog from it', async () => {
    const w = page()
    expect(w.find('[data-testid="empty-state"]').text()).toContain('No public URL yet')
    await w.find('[data-testid="empty-share"]').trigger('click')
    expect(useTunnels().state.share.open).toBe(true)
  })

  it('lists an open tunnel with project, port, URL, kind badge and countdown', () => {
    applyTunnels([tun(), tun({ id: 't2', kind: 'stable', port: 5432, project: 'api' })])
    const w = page()
    const rows = w.findAll('[data-testid="tunnel-row"]')
    expect(rows).toHaveLength(2)
    expect(rows[0].text()).toContain('acme')
    expect(rows[0].text()).toContain(':3000')
    expect(rows[0].find('[data-testid="tunnel-url"]').text()).toBe('https://abc.trycloudflare.com')
    expect(rows[0].find('[data-testid="kind-badge"]').text()).toBe('Random')
    expect(rows[1].find('[data-testid="kind-badge"]').text()).toBe('Stable')
    expect(rows[0].find('[data-testid="countdown"]').text()).toBe('3 h 12 min left')
    expect(w.find('[data-testid="empty-state"]').exists()).toBe(false)
  })

  it('refreshes the countdown every 30 seconds', async () => {
    applyTunnels([tun()])
    const w = page()
    vi.advanceTimersByTime(30 * 60_000)
    await flushPromises()
    expect(w.find('[data-testid="countdown"]').text()).toBe('2 h 42 min left')
  })

  it('copies the URL and opens it in the browser', async () => {
    applyTunnels([tun()])
    const w = page()
    await button(w, 'Copy').trigger('click')
    await flushPromises()
    expect(writeText).toHaveBeenCalledWith('https://abc.trycloudflare.com')
    await button(w, 'Open').trigger('click')
    expect(openUrl).toHaveBeenCalledWith('https://abc.trycloudflare.com')
  })

  it('calls extend for +1 h and stop for Stop', async () => {
    api.extend.mockResolvedValue(tun())
    api.stop.mockResolvedValue(undefined)
    applyTunnels([tun()])
    const w = page()
    await button(w, '+1 h').trigger('click')
    expect(api.extend).toHaveBeenCalledWith('t1')
    await button(w, 'Stop').trigger('click')
    expect(api.stop).toHaveBeenCalledWith('t1')
  })

  it('toasts a failed +1 h', async () => {
    api.extend.mockRejectedValue('Cannot extend')
    applyTunnels([tun()])
    const w = page()
    await button(w, '+1 h').trigger('click')
    await flushPromises()
    expect(messages()).toContain('error:Cannot extend')
  })

  it('shows the error and loads the output on Show output', async () => {
    api.output.mockResolvedValue(['line one', 'line two'])
    applyTunnels([tun({ status: 'error', url: null, error: 'cloudflared stopped.' })])
    const w = page()
    expect(w.find('[data-testid="tunnel-error"]').text()).toBe('cloudflared stopped.')
    expect(w.find('[data-testid="tunnel-output"]').exists()).toBe(false)
    await button(w, 'Show output').trigger('click')
    await flushPromises()
    expect(api.output).toHaveBeenCalledWith('t1')
    expect(w.find('[data-testid="tunnel-output"]').text()).toBe('line one\nline two')
  })

  it('warns when the port is absent from the Services snapshot', () => {
    applyTunnels([tun({ port: 3000 }), tun({ id: 't2', port: 7777 })])
    const w = page()
    const rows = w.findAll('[data-testid="tunnel-row"]')
    expect(rows[0].find('[data-testid="not-listening"]').exists()).toBe(false)
    expect(rows[1].find('[data-testid="not-listening"]').text()).toBe('Port 7777 is not listening')
  })

  it('does not warn before the first Services snapshot', () => {
    resetServices()
    applyTunnels([tun({ port: 7777 })])
    expect(page().find('[data-testid="not-listening"]').exists()).toBe(false)
  })

  it('lists stable URLs and deletes one after confirmation', async () => {
    useTunnels().state.status = { ...ready, stable: [{ cwd: '/x/acme', hostname: 'kl-acme.dev.acme.io' }] }
    const w = page()
    await flushPromises()
    api.deleteStable.mockResolvedValue(undefined)
    api.list.mockResolvedValue([])
    api.status.mockResolvedValue({ ...ready, stable: [] })
    const row = w.find('[data-testid="stable-row"]')
    expect(row.text()).toContain('kl-acme.dev.acme.io')
    expect(row.text()).toContain('/x/acme')
    await row.find('button').trigger('click')
    const req = useConfirm().request!
    expect(req.body).toBe('The DNS record kl-acme.dev.acme.io stays in Cloudflare; delete it from the Cloudflare dashboard.')
    expect(api.deleteStable).not.toHaveBeenCalled()
    answer({ ok: true, checked: false })
    await flushPromises()
    expect(api.deleteStable).toHaveBeenCalledWith('/x/acme')
    expect(w.find('[data-testid="stable-row"]').exists()).toBe(false)
  })

  it('does not delete when the confirmation is declined', async () => {
    useTunnels().state.status = { ...ready, stable: [{ cwd: '/x/acme', hostname: 'kl-acme.dev.acme.io' }] }
    const w = page()
    await w.find('[data-testid="stable-row"] button').trigger('click')
    answer({ ok: false, checked: false })
    await flushPromises()
    expect(api.deleteStable).not.toHaveBeenCalled()
  })

  it('shows no setup card when everything is ready', () => {
    expect(page().find('[data-testid="setup-card"]').exists()).toBe(false)
  })

  it('shows a copyable install command when cloudflared is missing', async () => {
    useTunnels().state.status = { ...ready, binary: null }
    const w = page()
    expect(w.find('[data-testid="install-command"]').text()).toBe('brew install cloudflared')
    await button(w, 'Copy').trigger('click')
    await flushPromises()
    expect(writeText).toHaveBeenCalledWith('brew install cloudflared')
  })

  it('offers Connect when not logged in, and rechecks the status on window focus', async () => {
    useTunnels().state.status = { ...ready, loggedIn: false }
    api.login.mockResolvedValue(undefined)
    api.status.mockResolvedValueOnce({ ...ready, loggedIn: false }).mockResolvedValue(ready)
    const w = page()
    await flushPromises()
    await button(w, 'Connect').trigger('click')
    expect(api.login).toHaveBeenCalled()
    expect(w.find('[data-testid="setup-card"]').exists()).toBe(true)
    window.dispatchEvent(new Event('focus'))
    await flushPromises()
    expect(api.status).toHaveBeenCalledTimes(2)
    expect(w.find('[data-testid="setup-card"]').exists()).toBe(false)
  })

  it('offers Open Settings when no domain is set', async () => {
    useTunnels().state.status = { ...ready, domain: null }
    const w = page()
    await button(w, 'Open Settings').trigger('click')
    expect(w.emitted('open-settings')).toHaveLength(1)
  })

  it('refreshes the cloudflared status when the page opens', async () => {
    useTunnels().state.status = { ...ready, domain: null }
    api.status.mockResolvedValue({ ...ready, stable: [{ cwd: '/x/acme', hostname: 'kl-acme.dev.acme.io' }] })
    const w = page()
    await flushPromises()
    expect(api.status).toHaveBeenCalledTimes(1)
    expect(w.find('[data-testid="setup-card"]').exists()).toBe(false)
    expect(w.find('[data-testid="stable-row"]').text()).toContain('kl-acme.dev.acme.io')
  })

  it('does not refresh the status on every tunnels change', async () => {
    const w = page()
    await flushPromises()
    applyTunnels([tun()])
    applyTunnels([tun({ expiresAt: NOW + 60 * 60_000 })])
    await flushPromises()
    expect(api.status).toHaveBeenCalledTimes(1)
    expect(w.findAll('[data-testid="tunnel-row"]')).toHaveLength(1)
  })

  it('says which project now serves the port when its folder changed', () => {
    const s = snapshot()
    s.processes = [proc({ ports: [{ port: 3000, responsive: true }], project: 'billing', cwd: '/x/billing' })]
    s.standalone = [ctr({ ports: [{ port: 5432, responsive: true }] })]
    s.compose = []
    applySnapshot(s)
    applyTunnels([tun({ port: 3000, cwd: '/x/acme' }), tun({ id: 't2', port: 5432, cwd: '/x/db' })])
    const rows = page().findAll('[data-testid="tunnel-row"]')
    expect(rows[0].find('[data-testid="moved"]').text()).toBe('Port 3000 now serves billing')
    expect(rows[0].find('[data-testid="not-listening"]').exists()).toBe(false)
    expect(rows[1].find('[data-testid="moved"]').exists()).toBe(false)
  })

  it('shows no moved notice when the same folder still serves the port', () => {
    const s = snapshot()
    s.processes = [proc({ ports: [{ port: 3000, responsive: true }], project: 'acme', cwd: '/x/acme' })]
    s.standalone = []
    s.compose = []
    applySnapshot(s)
    applyTunnels([tun({ cwd: '/x/acme' })])
    expect(page().find('[data-testid="moved"]').exists()).toBe(false)
  })

  it('asks for a valid domain when the domain setting is malformed', async () => {
    useTunnels().state.status = { ...ready, domain: 'dev..acme.io' }
    const w = page()
    expect(w.find('[data-testid="bad-domain"]').text()).toBe('Set a valid tunnel domain in Settings (for example dev.example.com).')
    await button(w, 'Open Settings').trigger('click')
    expect(w.emitted('open-settings')).toHaveLength(1)
  })
})
