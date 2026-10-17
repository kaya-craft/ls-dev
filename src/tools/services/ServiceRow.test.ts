import { beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import ServiceRow from './ServiceRow.vue'
import { applyTunnels, resetTunnels } from '../tunnels/store'
import type { Tunnel } from '../tunnels/types'
import { useToasts } from '../../ui/store/toasts'
import { ctr, proc } from '../../test/fixtures'
import type { ContainerService, ProcessService } from './types'

vi.mock('./api', () => ({ api: {} }))
vi.mock('./actions', () => ({ openBrowser: vi.fn() }))

const tun = (o: Partial<Tunnel> = {}): Tunnel => ({
  id: 't1', kind: 'quick', port: 4000, project: 'acme', cwd: null, url: 'https://abc.trycloudflare.com',
  status: 'open', error: null, startedAt: 1, expiresAt: 2, ...o,
})
const row = (service: ProcessService | ContainerService = proc()) => mount(ServiceRow, { props: { service, selected: false, cpuScale: 1, memScale: 1 } })
const writeText = vi.fn()

describe('ServiceRow public badge', () => {
  beforeEach(() => {
    resetTunnels()
    useToasts().items.splice(0)
    writeText.mockReset().mockResolvedValue(undefined)
    Object.assign(navigator, { clipboard: { writeText } })
  })

  it('is hidden without an open tunnel for one of the row ports', () => {
    expect(row().find('[data-testid="public-badge"]').exists()).toBe(false)
    applyTunnels([tun({ status: 'starting', url: null }), tun({ id: 'o', port: 9999 })])
    expect(row().find('[data-testid="public-badge"]').exists()).toBe(false)
  })

  it('shows for an open tunnel on a process port and copies the URL on click without selecting the row', async () => {
    applyTunnels([tun()])
    const w = row()
    const badge = w.find('[data-testid="public-badge"]')
    expect(badge.text()).toBe('Public')
    await badge.trigger('click')
    await flushPromises()
    expect(writeText).toHaveBeenCalledWith('https://abc.trycloudflare.com')
    expect(useToasts().items.map((t) => t.message)).toContain('Copied')
    expect(w.emitted('select')).toBeUndefined()
  })

  it('shows for a container port, also when it is a hidden extra port', () => {
    applyTunnels([tun({ port: 5433 })])
    const c = ctr({ ports: [{ port: 5432, responsive: true }, { port: 5433, responsive: true }, { port: 5434, responsive: null }] })
    expect(row(c).find('[data-testid="public-badge"]').exists()).toBe(true)
  })

  it('titles the badge with the URL to copy', () => {
    applyTunnels([tun({ cwd: '/w/shop/packages/backend' })])
    expect(row().find('[data-testid="public-badge"]').attributes('title')).toBe('Copy https://abc.trycloudflare.com')
  })

  it('titles the badge with the new project when the port now serves another folder', async () => {
    applyTunnels([tun({ cwd: '/x/acme' })])
    const w = row(proc({ project: 'billing', cwd: '/x/billing' }))
    const badge = w.find('[data-testid="public-badge"]')
    expect(badge.attributes('title')).toBe('Port 4000 now serves billing')
    await badge.trigger('click')
    await flushPromises()
    expect(writeText).toHaveBeenCalledWith('https://abc.trycloudflare.com')
  })

  it('does not check containers', () => {
    applyTunnels([tun({ port: 5432, cwd: '/x/acme' })])
    const c = ctr({ ports: [{ port: 5432, responsive: true }] })
    expect(row(c).find('[data-testid="public-badge"]').attributes('title')).toBe('Copy https://abc.trycloudflare.com')
  })
})
