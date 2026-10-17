import { describe, expect, it } from 'vitest'
import { nextSort, sinkStopped, sortServices } from './sort'
import { ctr, proc } from '../../test/fixtures'

describe('nextSort', () => {
  it('cycles desc, asc, off on the same key', () => {
    const a = nextSort(null, 'cpu')
    expect(a).toEqual({ key: 'cpu', dir: 'desc' })
    const b = nextSort(a, 'cpu')
    expect(b).toEqual({ key: 'cpu', dir: 'asc' })
    expect(nextSort(b, 'cpu')).toBeNull()
  })

  it('starts desc when the key changes', () => {
    expect(nextSort({ key: 'cpu', dir: 'asc' }, 'memory')).toEqual({ key: 'memory', dir: 'desc' })
  })
})

describe('sortServices', () => {
  const list = [
    proc({ id: 'proc:1:/a', cpuPercent: 5, memBytes: 30, uptimeSec: 10 }),
    proc({ id: 'proc:2:/b', cpuPercent: 50, memBytes: 10, uptimeSec: 30 }),
    proc({ id: 'proc:3:/c', cpuPercent: 20, memBytes: 20, uptimeSec: 20 }),
  ]
  const ids = (l: Array<{ id: string }>) => l.map((s) => s.id)

  it('sorts processes descending and ascending', () => {
    expect(ids(sortServices(list, { key: 'cpu', dir: 'desc' }))).toEqual(['proc:2:/b', 'proc:3:/c', 'proc:1:/a'])
    expect(ids(sortServices(list, { key: 'memory', dir: 'asc' }))).toEqual(['proc:2:/b', 'proc:3:/c', 'proc:1:/a'])
    expect(ids(sortServices(list, { key: 'uptime', dir: 'desc' }))).toEqual(['proc:2:/b', 'proc:3:/c', 'proc:1:/a'])
  })

  it('keeps the input order without a sort', () => {
    expect(ids(sortServices(list, null))).toEqual(ids(list))
  })

  it('puts null values last in both directions', () => {
    const cs = [
      ctr({ id: 'docker:n', cpuPercent: null }),
      ctr({ id: 'docker:lo', cpuPercent: 1 }),
      ctr({ id: 'docker:hi', cpuPercent: 9 }),
    ]
    expect(ids(sortServices(cs, { key: 'cpu', dir: 'desc' }))).toEqual(['docker:hi', 'docker:lo', 'docker:n'])
    expect(ids(sortServices(cs, { key: 'cpu', dir: 'asc' }))).toEqual(['docker:lo', 'docker:hi', 'docker:n'])
  })

  it('sorts containers by uptimeSec', () => {
    const cs = [ctr({ id: 'docker:a', uptimeSec: 5 }), ctr({ id: 'docker:b', uptimeSec: 50 }), ctr({ id: 'docker:c', uptimeSec: null })]
    expect(ids(sortServices(cs, { key: 'uptime', dir: 'desc' }))).toEqual(['docker:b', 'docker:a', 'docker:c'])
  })

  it('treats undefined like null (stale backend without uptimeSec)', () => {
    const cs = [
      ctr({ id: 'docker:u', uptimeSec: undefined as unknown as null }),
      ctr({ id: 'docker:a', uptimeSec: 5 }),
      ctr({ id: 'docker:b', uptimeSec: 50 }),
    ]
    expect(ids(sortServices(cs, { key: 'uptime', dir: 'desc' }))).toEqual(['docker:b', 'docker:a', 'docker:u'])
    expect(ids(sortServices(cs, { key: 'uptime', dir: 'asc' }))).toEqual(['docker:a', 'docker:b', 'docker:u'])
  })

  it('is stable for ties', () => {
    const ties = [proc({ id: 'proc:1:/a', cpuPercent: 3 }), proc({ id: 'proc:2:/b', cpuPercent: 3 }), proc({ id: 'proc:3:/c', cpuPercent: 3 })]
    expect(ids(sortServices(ties, { key: 'cpu', dir: 'desc' }))).toEqual(['proc:1:/a', 'proc:2:/b', 'proc:3:/c'])
    expect(ids(sortServices(ties, { key: 'cpu', dir: 'asc' }))).toEqual(['proc:1:/a', 'proc:2:/b', 'proc:3:/c'])
  })

  it('does not mutate its input', () => {
    const before = ids(list)
    const out = sortServices(list, { key: 'cpu', dir: 'desc' })
    expect(ids(list)).toEqual(before)
    expect(out).not.toBe(list)
    expect(sortServices(list, null)).not.toBe(list)
  })
})

describe('sinkStopped', () => {
  const ids = (l: Array<{ id: string }>) => l.map((x) => x.id)

  it('moves non-running containers below running ones, keeping order inside each group', () => {
    const cs = [
      ctr({ id: 'docker:a', state: 'exited' }),
      ctr({ id: 'docker:b' }),
      ctr({ id: 'docker:c', state: 'paused' }),
      ctr({ id: 'docker:d' }),
    ]
    expect(ids(sinkStopped(cs))).toEqual(['docker:b', 'docker:d', 'docker:a', 'docker:c'])
  })

  it('leaves processes in place and does not mutate its input', () => {
    const ps = [proc({ id: 'proc:1:/a' }), proc({ id: 'proc:2:/b' })]
    const out = sinkStopped(ps)
    expect(ids(out)).toEqual(['proc:1:/a', 'proc:2:/b'])
    expect(out).not.toBe(ps)
  })
})
