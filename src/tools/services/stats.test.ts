import { describe, expect, it } from 'vitest'
import { pushHistory, sectionKey, sectionStats, type SectionStats } from './stats'
import { ctr, proc, snapshot } from '../../test/fixtures'

describe('sectionKey', () => {
  it('names every section', () => {
    expect(sectionKey('all')).toBe('all')
    expect(sectionKey('dev')).toBe('dev')
    expect(sectionKey('standalone')).toBe('standalone')
    expect(sectionKey({ compose: 'acme' })).toBe('compose:acme')
  })
})

describe('sectionStats', () => {
  it('counts ports of processes and running containers, and responding ports', () => {
    const s = snapshot()
    const st = sectionStats(s, false)
    expect(st.ports).toBe(4)
    expect(st.responding).toBe(3)
    expect(st.running).toBe(4)
    expect(st.stopped).toBe(1)
  })
  it('sums CPU and memory, null when nothing reports', () => {
    const st = sectionStats({ processes: [proc({ cpuPercent: 1.5, memBytes: 100 })], compose: [], standalone: [ctr({ state: 'exited', cpuPercent: null, memBytes: null })] }, false)
    expect(st.cpu).toBe(1.5)
    expect(st.mem).toBe(100)
    expect(sectionStats({ processes: [], compose: [], standalone: [] }, false)).toMatchObject({ ports: 0, cpu: null, mem: null })
  })
})

describe('pushHistory', () => {
  it('keeps the last 30 points per key', () => {
    const h: Record<string, { cpu: number[]; mem: number[] }> = {}
    for (let i = 0; i < 35; i++) {
      const stats: SectionStats = { ports: 0, responding: 0, running: 0, stopped: 0, cpu: i, mem: i * 10, isCompose: false }
      pushHistory(h, 'all', stats)
    }
    expect(h.all.cpu).toHaveLength(30)
    expect(h.all.cpu[0]).toBe(5)
    expect(h.all.mem[h.all.mem.length - 1]).toBe(340)
  })

  it('skips the push for a series whose stat is null', () => {
    const h: Record<string, { cpu: number[]; mem: number[] }> = {}
    const base: SectionStats = { ports: 0, responding: 0, running: 0, stopped: 0, cpu: 2, mem: 100, isCompose: false }
    pushHistory(h, 'all', base)
    pushHistory(h, 'all', { ...base, cpu: null, mem: 200 })
    expect(h.all.cpu).toEqual([2])
    expect(h.all.mem).toEqual([100, 200])
  })
})
