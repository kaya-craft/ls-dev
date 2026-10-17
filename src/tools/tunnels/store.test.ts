import { beforeEach, describe, expect, it, vi } from 'vitest'
import { tunnelsApi } from './api'
import { applyTunnels, closeShare, forPort, movedNotice, openShare, refreshStatus, resetTunnels, timeLeft, useTunnels, validDomain } from './store'
import type { Tunnel, TunnelsStatus } from './types'

vi.mock('./api', () => ({ tunnelsApi: { status: vi.fn() } }))
const api = vi.mocked(tunnelsApi)

const t = (o: Partial<Tunnel> = {}): Tunnel => ({
  id: 'a', kind: 'quick', port: 3000, project: 'acme', cwd: '/x/acme', url: null,
  status: 'open', error: null, startedAt: 1000, expiresAt: 2000, ...o,
})

describe('tunnels store', () => {
  beforeEach(() => resetTunnels())

  it('applyTunnels replaces the list', () => {
    applyTunnels([t({ id: 'a' }), t({ id: 'b' })])
    applyTunnels([t({ id: 'c' })])
    expect(useTunnels().state.tunnels.map((x) => x.id)).toEqual(['c'])
  })

  it('forPort prefers open, then starting, over error entries', () => {
    applyTunnels([
      t({ id: 'err', status: 'error', startedAt: 9 }),
      t({ id: 'start', status: 'starting' }),
      t({ id: 'open', status: 'open' }),
      t({ id: 'other', port: 4000 }),
    ])
    expect(forPort(3000)?.id).toBe('open')
    applyTunnels([t({ id: 'err', status: 'error' }), t({ id: 'start', status: 'starting' })])
    expect(forPort(3000)?.id).toBe('start')
  })

  it('forPort returns the latest error entry when only errors exist, undefined when none', () => {
    applyTunnels([t({ id: 'old', status: 'error', startedAt: 1 }), t({ id: 'new', status: 'error', startedAt: 5 })])
    expect(forPort(3000)?.id).toBe('new')
    expect(forPort(5000)).toBeUndefined()
  })

  it('timeLeft formats hours, minutes and the last minute', () => {
    const now = 1_000_000
    const at = (ms: number) => t({ expiresAt: now + ms })
    expect(timeLeft(at((3 * 60 + 12) * 60_000 + 5_000), now)).toBe('3 h 12 min left')
    expect(timeLeft(at(12 * 60_000), now)).toBe('12 min left')
    expect(timeLeft(at(59_000), now)).toBe('less than 1 min left')
    expect(timeLeft(at(-5_000), now)).toBe('less than 1 min left')
  })

  it('openShare opens the dialog with the target, closeShare closes it', () => {
    openShare({ port: 3000, project: 'acme', cwd: '/x/acme' })
    expect(useTunnels().state.share).toEqual({ open: true, port: 3000, project: 'acme', cwd: '/x/acme' })
    closeShare()
    expect(useTunnels().state.share.open).toBe(false)
  })

  it('openShare without a target opens with no port', () => {
    openShare()
    expect(useTunnels().state.share).toEqual({ open: true, port: null, project: '', cwd: null })
  })

  it('validDomain follows the backend rule', () => {
    for (const ok of ['dev.example.com', 'a-b.example.com', 'localhost']) expect(validDomain(ok), ok).toBe(true)
    for (const bad of ['', 'dev..example.com', 'dev example.com', 'dev_example.com', 'exémple.com', 'dev.example.com/x', '--url', 'Dev.example.com']) {
      expect(validDomain(bad), bad).toBe(false)
    }
  })

  it('movedNotice names the project now listening on the port of a tunnel', () => {
    const other = { cwd: '/x/other', project: 'other', ports: [{ port: 3000 }] }
    const same = { cwd: '/x/acme', project: 'acme', ports: [{ port: 3000 }] }
    expect(movedNotice(t(), [other])).toBe('Port 3000 now serves other')
    expect(movedNotice(t(), [same])).toBeNull()
    expect(movedNotice(t(), [{ ...other, cwd: null }])).toBeNull()
    expect(movedNotice(t({ cwd: null }), [other])).toBeNull()
    expect(movedNotice(t({ status: 'error' }), [other])).toBeNull()
    expect(movedNotice(t(), [{ ...other, ports: [{ port: 4000 }] }])).toBeNull()
  })

  it('refreshStatus stores the status and keeps the previous one on failure', async () => {
    const ready: TunnelsStatus = { binary: '/b', loggedIn: true, domain: 'dev.acme.io', stable: [] }
    api.status.mockResolvedValueOnce(ready)
    await refreshStatus()
    expect(useTunnels().state.status).toEqual(ready)
    api.status.mockRejectedValueOnce('boom')
    await refreshStatus()
    expect(useTunnels().state.status).toEqual(ready)
  })
})
