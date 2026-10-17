import { describe, expect, it } from 'vitest'
import { iconFor, meterFraction, meterScale, splitName, subline } from './rowText'
import { ctr, proc } from '../../test/fixtures'

describe('iconFor', () => {
  it.each([
    ['node', 'N'],
    ['bun', 'B'],
    ['deno', 'D'],
  ])('process runtime %s', (runtime, label) => {
    expect(iconFor(proc({ runtime }))).toEqual({ label, tone: 'node' })
  })

  it.each([
    ['postgres:16', 'Pg', 'pg'],
    ['pgvector/pgvector:pg17', 'Pg', 'pg'],
    ['redis:7-alpine', 'Rd', 'rd'],
    ['valkey/valkey:8', 'Rd', 'rd'],
    ['minio/minio', 'S3', 's3'],
    ['rustfs/rustfs', 'S3', 's3'],
    ['axllent/mailpit', '@', 'mail'],
    ['mailhog/mailhog', '@', 'mail'],
    ['localstack/localstack:3', 'Ls', 'ls'],
    ['amazon/s3-mock', 'S3', 's3'],
  ])('container image %s', (image, label, tone) => {
    expect(iconFor(ctr({ image }))).toEqual({ label, tone })
  })

  it('falls back to the first two letters of the name', () => {
    expect(iconFor(ctr({ name: 'zookeeper', image: 'confluentinc/cp-zookeeper' }))).toEqual({ label: 'Zo', tone: 'off' })
  })

  it('exited redis keeps Rd with tone off', () => {
    expect(iconFor(ctr({ image: 'redis:7', state: 'exited' }))).toEqual({ label: 'Rd', tone: 'off' })
  })

  it('unknown process runtime falls back to its first two letters with tone off', () => {
    expect(iconFor(proc({ runtime: 'python' }))).toEqual({ label: 'Py', tone: 'off' })
  })

  it('keeps the label of a stopped container with tone off', () => {
    expect(iconFor(ctr({ state: 'exited' }))).toEqual({ label: 'Pg', tone: 'off' })
  })
})

describe('meterScale', () => {
  it('floors cpu at 10', () => {
    expect(meterScale('cpu', [2, null])).toBe(10)
  })
  it('cpu with no values or only nulls is 10', () => {
    expect(meterScale('cpu', [])).toBe(10)
    expect(meterScale('cpu', [null, null])).toBe(10)
  })
  it('mem with no values is 640 MiB', () => {
    expect(meterScale('mem', [])).toBe(640 * 1024 * 1024)
  })
  it('mem above the floor is kept', () => {
    expect(meterScale('mem', [2 * 640 * 1024 * 1024])).toBe(2 * 640 * 1024 * 1024)
  })
  it('uses the max cpu above the floor', () => {
    expect(meterScale('cpu', [42])).toBe(42)
  })
  it('floors mem at 640 MiB', () => {
    expect(meterScale('mem', [100])).toBe(640 * 1024 * 1024)
  })
})

describe('meterFraction', () => {
  it('returns 0 for null', () => expect(meterFraction(null, 10)).toBe(0))
  it('returns 0 for a zero scale', () => expect(meterFraction(5, 0)).toBe(0))
  it('returns the ratio', () => expect(meterFraction(5, 10)).toBe(0.5))
  it('clamps to 1', () => expect(meterFraction(20, 10)).toBe(1))
  it('returns 0 for NaN or infinite inputs', () => {
    expect(meterFraction(NaN, 10)).toBe(0)
    expect(meterFraction(5, NaN)).toBe(0)
    expect(meterFraction(Infinity, 10)).toBe(0)
    expect(meterFraction(5, Infinity)).toBe(0)
  })
  it('clamps negatives to 0', () => expect(meterFraction(-3, 10)).toBe(0))
})

describe('splitName', () => {
  it('splits on the last slash', () => {
    expect(splitName('shop/packages/app')).toEqual({ prefix: 'shop/packages/', last: 'app' })
  })
  it('handles a name without slash', () => {
    expect(splitName('blog')).toEqual({ prefix: '', last: 'blog' })
  })
})

describe('subline', () => {
  it('process: script and root pid', () => {
    expect(subline(proc(), { narrow: false })).toBe('pnpm --filter @acme/backend dev · 29414')
  })
  it('running container: image', () => {
    expect(subline(ctr(), { narrow: false })).toBe('pgvector/pgvector:pg17')
  })
  it('stopped container: image and status', () => {
    expect(subline(ctr({ state: 'exited', status: 'Exited (0) 3 days ago', uptimeSec: null }), { narrow: false })).toBe(
      'pgvector/pgvector:pg17 · Exited (0) 3 days ago',
    )
  })
  it('narrow does not append an uptime for a stopped container', () => {
    expect(subline(ctr({ state: 'exited', status: 'Exited (0) 3 days ago', uptimeSec: null }), { narrow: true })).toBe(
      'pgvector/pgvector:pg17 · Exited (0) 3 days ago',
    )
  })
  it('narrow appends the uptime for a process', () => {
    expect(subline(proc({ uptimeSec: 3600 }), { narrow: true })).toBe('pnpm --filter @acme/backend dev · 29414 · 1h0m')
  })
  it('narrow appends the uptime for a running container', () => {
    expect(subline(ctr({ uptimeSec: 120 }), { narrow: true })).toBe('pgvector/pgvector:pg17 · 2m')
  })
  it('narrow omits an unknown uptime', () => {
    expect(subline(proc({ uptimeSec: null }), { narrow: true })).toBe('pnpm --filter @acme/backend dev · 29414')
  })
})
