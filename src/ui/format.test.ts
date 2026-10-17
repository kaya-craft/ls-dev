import { describe, expect, it } from 'vitest'
import { formatAgo, formatBytes, formatCpu, formatUptime } from './format'

describe('formatUptime', () => {
  it('picks the two largest units', () => {
    expect(formatUptime(45)).toBe('45s')
    expect(formatUptime(12 * 60 + 5)).toBe('12m')
    expect(formatUptime(3 * 3600 + 27 * 60)).toBe('3h27m')
    expect(formatUptime(9 * 86400 + 22 * 3600)).toBe('9d22h')
  })
  it('shows a dash when unknown', () => {
    expect(formatUptime(null)).toBe('—')
  })
})

describe('formatBytes', () => {
  it('uses binary units', () => {
    expect(formatBytes(512)).toBe('512 B')
    expect(formatBytes(300 * 1024 * 1024)).toBe('300 MB')
    expect(formatBytes(1.25 * 1024 ** 3)).toBe('1.3 GB')
    expect(formatBytes(null)).toBe('—')
  })
})

describe('formatCpu', () => {
  it('keeps one decimal', () => {
    expect(formatCpu(3.14159)).toBe('3.1%')
    expect(formatCpu(null)).toBe('—')
  })
})

describe('formatAgo', () => {
  const MIN = 60
  const H = 3600
  const D = 86400
  it('picks the largest unit and pluralises', () => {
    expect(formatAgo(5)).toBe('just now')
    expect(formatAgo(5 * MIN)).toBe('5 min ago')
    expect(formatAgo(H)).toBe('1 hour ago')
    expect(formatAgo(7 * H)).toBe('7 hours ago')
    expect(formatAgo(D)).toBe('1 day ago')
    expect(formatAgo(3 * D)).toBe('3 days ago')
    expect(formatAgo(45 * D)).toBe('1 month ago')
    expect(formatAgo(200 * D)).toBe('6 months ago')
    expect(formatAgo(800 * D)).toBe('2 years ago')
  })
  it('shows a dash when unknown and treats a future date as just now', () => {
    expect(formatAgo(null)).toBe('—')
    expect(formatAgo(-30)).toBe('just now')
  })
})
