import { beforeEach, describe, expect, it, vi } from 'vitest'
import { clearLogs, handleChunk, handleClosed, openLogs, resetLogs, setPaused, useLogs, MAX_LINES } from './logs'
import { api } from '../api'
import { ctr, proc } from '../../../test/fixtures'

vi.mock('../api', () => ({ api: { logsOpen: vi.fn().mockResolvedValue(undefined), logsClose: vi.fn() } }))

describe('logs store', () => {
  beforeEach(() => { resetLogs(); vi.clearAllMocks() })

  it('does not open a stream for a process started outside LsDev', async () => {
    await openLogs(proc({ managed: false }))
    expect(api.logsOpen).not.toHaveBeenCalled()
    expect(useLogs().unavailable).toBe(true)
  })

  it('ignores chunks of another service', async () => {
    await openLogs(ctr())
    handleChunk({ id: 'docker:other', lines: [{ stream: 'stdout', line: 'x' }] })
    handleChunk({ id: 'docker:aaa', lines: [{ stream: 'stdout', line: 'y' }] })
    expect(useLogs().lines.map((l) => l.line)).toEqual(['y'])
  })

  it('caps the buffer', async () => {
    await openLogs(ctr())
    const many = Array.from({ length: MAX_LINES + 10 }, (_, i) => ({ stream: 'stdout' as const, line: String(i) }))
    handleChunk({ id: 'docker:aaa', lines: many })
    expect(useLogs().lines).toHaveLength(MAX_LINES)
    expect(useLogs().lines[0].line).toBe('10')
  })

  it('buffers while paused and flushes on resume', async () => {
    await openLogs(ctr())
    setPaused(true)
    handleChunk({ id: 'docker:aaa', lines: [{ stream: 'stdout', line: 'a' }] })
    expect(useLogs().lines).toHaveLength(0)
    setPaused(false)
    expect(useLogs().lines.map((l) => l.line)).toEqual(['a'])
  })

  it('adds a marker line when the stream closes, and clears on demand', async () => {
    await openLogs(ctr())
    handleClosed({ id: 'docker:aaa', reason: 'stream ended' })
    const lines = useLogs().lines
    expect(lines[lines.length - 1]?.line).toContain('stream closed')
    clearLogs()
    expect(useLogs().lines).toHaveLength(0)
  })

  it('bumps version on every append even when the buffer is capped', async () => {
    await openLogs(ctr())
    const many = Array.from({ length: MAX_LINES }, (_, i) => ({ stream: 'stdout' as const, line: String(i) }))
    handleChunk({ id: 'docker:aaa', lines: many })
    const before = useLogs().version
    handleChunk({ id: 'docker:aaa', lines: [{ stream: 'stdout', line: 'new' }] })
    expect(useLogs().lines).toHaveLength(MAX_LINES)
    expect(useLogs().version).toBeGreaterThan(before)
  })
})
