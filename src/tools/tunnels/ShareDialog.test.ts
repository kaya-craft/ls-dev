import { mount, flushPromises } from '@vue/test-utils'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { tunnelsApi } from './api'
import ShareDialog from './ShareDialog.vue'
import { applyTunnels, closeShare, openShare, resetTunnels, useTunnels } from './store'
import type { Tunnel, TunnelsStatus } from './types'
import { useSettings } from '../../ui/store/settings'
import { useToasts } from '../../ui/store/toasts'
import { applySnapshot, resetServices } from '../services/store/services'
import { ctr, proc, snapshot } from '../../test/fixtures'

vi.mock('./api', () => ({ tunnelsApi: { open: vi.fn(), suggestName: vi.fn(), status: vi.fn() } }))
const api = vi.mocked(tunnelsApi)

const ready: TunnelsStatus = { binary: '/opt/homebrew/bin/cloudflared', loggedIn: true, domain: 'dev.acme.io', stable: [] }
const tun = (o: Partial<Tunnel> = {}): Tunnel => ({
  id: 't1', kind: 'quick', port: 3000, project: 'acme', cwd: '/x/acme', url: null,
  status: 'starting', error: null, startedAt: 1, expiresAt: 2, ...o,
})
const messages = () => useToasts().items.map((t) => `${t.kind}:${t.message}`)
const writeText = vi.fn()

const mounted: Array<{ unmount: () => void }> = []
function track<T extends { unmount: () => void }>(w: T): T {
  mounted.push(w)
  return w
}
afterEach(() => mounted.splice(0).forEach((w) => w.unmount()))

function setup() {
  openShare({ port: 3000, project: 'acme', cwd: '/x/acme' })
  return track(mount(ShareDialog, { attachTo: document.body }))
}
const pickStable = async (w: ReturnType<typeof setup>) => {
  await w.find('input[value="stable"]').setValue(true)
  await flushPromises()
}

beforeEach(() => {
  vi.resetAllMocks()
  resetTunnels()
  resetServices()
  useToasts().items.splice(0)
  useTunnels().state.status = ready
  useSettings().settings = { tunnelHours: 4 } as never
  api.suggestName.mockResolvedValue({ subdomain: 'kl-acme', remembered: false })
  api.status.mockImplementation(async () => useTunnels().state.status as TunnelsStatus)
  Object.assign(navigator, { clipboard: { writeText } })
  writeText.mockResolvedValue(undefined)
  document.body.innerHTML = ''
})

describe('ShareDialog', () => {
  it('renders nothing while closed and shows the warning when open', () => {
    const closed = track(mount(ShareDialog))
    expect(closed.find('[role="dialog"]').exists()).toBe(false)
    const w = setup()
    expect(w.find('[data-testid="share-warning"]').text()).toBe('Anyone with the link can reach this server until it closes.')
  })

  it.each([
    [{ binary: null }, 'cloudflared is not installed. Run: brew install cloudflared'],
    [{ loggedIn: false }, 'Connect cloudflared to a Cloudflare account first.'],
    [{ domain: null }, 'Set a tunnel domain in Settings first.'],
    [{ domain: 'dev..acme.io' }, 'Set a valid tunnel domain in Settings (for example dev.example.com).'],
    [{ domain: 'dev_acme.io' }, 'Set a valid tunnel domain in Settings (for example dev.example.com).'],
  ])('disables Stable with its reason (%j)', (patch, reason) => {
    useTunnels().state.status = { ...ready, ...patch }
    const w = setup()
    expect(w.find('input[value="stable"]').attributes('disabled')).toBeDefined()
    expect(w.find('[data-testid="stable-reason"]').text()).toBe(reason)
  })

  it('checks the reasons in order: binary first', () => {
    useTunnels().state.status = { binary: null, loggedIn: false, domain: null, stable: [] }
    const w = setup()
    expect(w.find('[data-testid="stable-reason"]').text()).toContain('brew install cloudflared')
  })

  it('prefills the stable name and the domain suffix', async () => {
    const w = setup()
    await pickStable(w)
    expect(api.suggestName).toHaveBeenCalledWith('/x/acme', 'acme')
    expect((w.find('#share-name').element as HTMLInputElement).value).toBe('kl-acme')
    expect(w.find('[data-testid="share-suffix"]').text()).toBe('.dev.acme.io')
    expect(w.find('#share-name').attributes('readonly')).toBeUndefined()
  })

  it('makes a remembered name read-only', async () => {
    api.suggestName.mockResolvedValue({ subdomain: 'kl-acme', remembered: true })
    const w = setup()
    await pickStable(w)
    expect(w.find('#share-name').attributes('readonly')).toBeDefined()
  })

  it('blocks Share on an invalid typed name', async () => {
    const w = setup()
    await pickStable(w)
    await w.find('#share-name').setValue('Bad_Name')
    await w.find('form').trigger('submit')
    expect(api.open).not.toHaveBeenCalled()
    expect(w.find('[data-testid="name-error"]').text()).toBe('Use lowercase letters, digits and dashes (63 characters at most).')
  })

  it('defaults the duration to settings.tunnelHours', () => {
    useSettings().settings = { tunnelHours: 8 } as never
    const w = setup()
    expect((w.find('#share-hours').element as HTMLSelectElement).value).toBe('8')
  })

  it('calls open with the exact request, closes and toasts', async () => {
    api.open.mockResolvedValue(tun())
    const w = setup()
    await w.find('#share-hours').setValue('24')
    await w.find('form').trigger('submit')
    await flushPromises()
    expect(api.open).toHaveBeenCalledWith({ port: 3000, project: 'acme', cwd: '/x/acme', kind: 'quick', subdomain: null, hours: 24 })
    expect(useTunnels().state.share.open).toBe(false)
    expect(messages()).toContain('success:Opening tunnel…')
  })

  it('sends the stable subdomain', async () => {
    api.open.mockResolvedValue(tun({ kind: 'stable' }))
    const w = setup()
    await pickStable(w)
    await w.find('form').trigger('submit')
    await flushPromises()
    expect(api.open).toHaveBeenCalledWith(expect.objectContaining({ kind: 'stable', subdomain: 'kl-acme', hours: 4 }))
  })

  it('toasts a backend refusal as is and stays open', async () => {
    api.open.mockRejectedValue('Port 3000 is not listening')
    const w = setup()
    await w.find('form').trigger('submit')
    await flushPromises()
    expect(messages()).toContain('error:Port 3000 is not listening')
    expect(useTunnels().state.share.open).toBe(true)
  })

  it('copies the URL when the tunnel turns open', async () => {
    api.open.mockResolvedValue(tun())
    const w = setup()
    await w.find('form').trigger('submit')
    await flushPromises()
    expect(writeText).not.toHaveBeenCalled()
    applyTunnels([tun({ status: 'open', url: 'https://abc.trycloudflare.com' })])
    await flushPromises()
    expect(writeText).toHaveBeenCalledWith('https://abc.trycloudflare.com')
    expect(messages()).toContain('success:Tunnel open — URL copied')
    applyTunnels([tun({ status: 'open', url: 'https://abc.trycloudflare.com', expiresAt: 3 })])
    await flushPromises()
    expect(writeText).toHaveBeenCalledTimes(1)
  })

  it('copies the URL when the open event arrived before the invoke response', async () => {
    api.open.mockImplementation(async () => {
      applyTunnels([tun({ status: 'open', url: 'https://abc.trycloudflare.com' })])
      return tun()
    })
    const w = setup()
    await w.find('form').trigger('submit')
    await flushPromises()
    expect(writeText).toHaveBeenCalledWith('https://abc.trycloudflare.com')
    expect(writeText).toHaveBeenCalledTimes(1)
  })

  it('clears a stale remembered name while the name of another port loads', async () => {
    const s = snapshot()
    s.processes = [proc({ ports: [{ port: 3000, responsive: true }], project: 'acme', cwd: '/x/acme' }), proc({ id: 'p2', ports: [{ port: 4000, responsive: true }], project: 'api', cwd: '/x/api' })]
    s.standalone = []
    s.compose = []
    applySnapshot(s)
    api.suggestName.mockResolvedValueOnce({ subdomain: 'kl-acme', remembered: true })
    const w = setup()
    await pickStable(w)
    expect(w.find('#share-name').attributes('readonly')).toBeDefined()
    api.suggestName.mockReturnValueOnce(new Promise(() => {}))
    await w.find('#share-port').setValue('4000')
    await flushPromises()
    expect((w.find('#share-name').element as HTMLInputElement).value).toBe('')
    expect(w.find('#share-name').attributes('readonly')).toBeUndefined()
    await w.find('form').trigger('submit')
    expect(api.open).not.toHaveBeenCalled()
    expect(w.find('[data-testid="name-error"]').exists()).toBe(true)
  })

  it('shows "Checking cloudflared…" while the status is unknown', async () => {
    useTunnels().state.status = null
    api.status.mockReturnValue(new Promise(() => {}))
    const w = setup()
    expect(w.find('[data-testid="stable-reason"]').text()).toBe('Checking cloudflared…')
    expect(w.find('input[value="stable"]').attributes('disabled')).toBeDefined()
  })

  it('refreshes the cloudflared status each time the dialog opens', async () => {
    useTunnels().state.status = { ...ready, binary: null }
    api.status.mockResolvedValue(ready)
    const w = setup()
    await flushPromises()
    expect(api.status).toHaveBeenCalledTimes(1)
    expect(w.find('input[value="stable"]').attributes('disabled')).toBeUndefined()
    closeShare()
    await flushPromises()
    openShare({ port: 3000, project: 'acme', cwd: '/x/acme' })
    await flushPromises()
    expect(api.status).toHaveBeenCalledTimes(2)
  })

  it('keeps the previous status when the refresh fails', async () => {
    api.status.mockRejectedValue('boom')
    const w = setup()
    await flushPromises()
    expect(useTunnels().state.status).toEqual(ready)
    expect(w.find('input[value="stable"]').attributes('disabled')).toBeUndefined()
  })

  it('toasts the error when the tunnel ends in error', async () => {
    api.open.mockResolvedValue(tun())
    const w = setup()
    await w.find('form').trigger('submit')
    await flushPromises()
    applyTunnels([tun({ status: 'error', error: 'cloudflared stopped.' })])
    await flushPromises()
    expect(messages()).toContain('error:cloudflared stopped.')
  })

  it('lists listening processes and running containers when opened without a port', async () => {
    const s = snapshot()
    s.processes = [proc({ ports: [{ port: 4000, responsive: true }], project: 'api', cwd: '/w/api' })]
    s.standalone = [ctr({ name: 'pg', ports: [{ port: 5432, responsive: true }] }), ctr({ id: 'docker:b', name: 'stopped', state: 'exited', ports: [{ port: 6379, responsive: null }] })]
    s.compose = []
    applySnapshot(s)
    openShare()
    const w = track(mount(ShareDialog, { attachTo: document.body }))
    await flushPromises()
    expect(w.findAll('#share-port option').map((o) => o.text())).toEqual([':4000 — api', ':5432 — pg'])
    api.open.mockResolvedValue(tun())
    await w.find('#share-port').setValue('5432')
    await w.find('form').trigger('submit')
    await flushPromises()
    expect(api.open).toHaveBeenCalledWith(expect.objectContaining({ port: 5432, project: 'pg', cwd: null }))
  })

  it('closes on Escape', async () => {
    const w = setup()
    await w.find('[role="dialog"]').trigger('keydown', { key: 'Escape' })
    expect(useTunnels().state.share.open).toBe(false)
  })

  it('shows the full remembered hostname, also under another domain', async () => {
    useTunnels().state.status = { ...ready, stable: [{ cwd: '/x/acme', hostname: 'kl-acme.dev.old.io' }] }
    api.suggestName.mockResolvedValue({ subdomain: 'kl-acme', remembered: true })
    const w = setup()
    await pickStable(w)
    expect(w.find('[data-testid="share-hostname"]').text()).toBe('kl-acme.dev.old.io')
    expect(w.find('[data-testid="share-suffix"]').exists()).toBe(false)
    expect(w.find('#share-name').exists()).toBe(false)
  })

  it('refreshes the status after a stable URL opens, not after a random one', async () => {
    const w = setup()
    await flushPromises()
    expect(api.status).toHaveBeenCalledTimes(1)
    api.open.mockResolvedValue(tun())
    await w.find('form').trigger('submit')
    await flushPromises()
    expect(api.status).toHaveBeenCalledTimes(1)

    openShare({ port: 3000, project: 'acme', cwd: '/x/acme' })
    await flushPromises()
    expect(api.status).toHaveBeenCalledTimes(2)
    api.open.mockResolvedValue(tun({ kind: 'stable' }))
    await pickStable(w)
    await w.find('form').trigger('submit')
    await flushPromises()
    expect(api.status).toHaveBeenCalledTimes(3)
  })
})
