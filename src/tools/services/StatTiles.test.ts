import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { mount } from '@vue/test-utils'
import StatTiles from './StatTiles.vue'
import Sparkline from './Sparkline.vue'
import { applySnapshot, resetServices, useServices } from './store/services'
import { formatBytes, formatCpu } from '../../ui/format'
import { resetNavigation, setServicesSection } from '../../shell/navigation'
import { isNarrow } from '../../shell/viewport'
import { snapshot } from '../../test/fixtures'

const tiles = (w: ReturnType<typeof mount>) =>
  w.findAll('[data-testid="tile"]').map((t) => ({
    label: t.find('.k').text(),
    value: t.find('.v').text().replace(/\s+/g, ' '),
    secondary: t.find('.v small').exists() ? t.find('.v small').text() : null,
  }))

describe('StatTiles', () => {
  beforeEach(() => {
    resetServices()
    resetNavigation()
    isNarrow.value = false
  })
  afterEach(() => {
    isNarrow.value = false
  })

  it('shows four tiles scoped to All with values from the stats', () => {
    applySnapshot(snapshot())
    const t = tiles(mount(StatTiles))
    expect(t.map((x) => x.label)).toEqual(['Listening ports', 'Services', 'CPU', 'Memory'])
    expect(t[0].value).toMatch(/^\d+/)
    expect(t[0].secondary).toMatch(/^\d+ responding$/)
    expect(t[1].secondary).toMatch(/^running · \d+ stopped$/)
    expect(t[2].value).toMatch(/^\d+\.\d%$/)
    expect(t[3].value).toMatch(/^\d+(\.\d)? ?(KB|MB|GB)$/)
  })

  it('shows exact values from stats and passes history to the sparklines', () => {
    applySnapshot(snapshot())
    const { stats, history } = useServices()
    const w = mount(StatTiles)
    const t = tiles(w)
    expect(t[0].value).toBe(`${stats.value.ports} ${stats.value.responding} responding`)
    expect(t[1].value).toBe(`${stats.value.running} running · ${stats.value.stopped} stopped`)
    expect(t[2].value).toBe(formatCpu(stats.value.cpu))
    expect(t[3].value).toBe(formatBytes(stats.value.mem))
    const lines = w.findAllComponents(Sparkline)
    expect(lines[0].props('series')).toEqual(history.value.cpu)
    expect(lines[1].props('series')).toEqual(history.value.mem)
  })

  it('narrow: compact tiles with Ports label and "n up" / "· n off" secondaries', () => {
    applySnapshot(snapshot())
    isNarrow.value = true
    const { stats } = useServices()
    const w = mount(StatTiles)
    const t = tiles(w)
    expect(t[0].label).toBe('Ports')
    expect(t[0].secondary).toBe(`${stats.value.responding} up`)
    expect(t[1].secondary).toBe(`· ${stats.value.stopped} off`)
    expect(w.find('[data-testid="tile"]').classes()).toEqual(expect.arrayContaining(['flex-col', 'rounded-xl']))
  })

  it('computes the numbers from the fixture', () => {
    applySnapshot(snapshot())
    const t = tiles(mount(StatTiles))
    // 2 dev processes with 1 port each, running compose/standalone containers add theirs.
    expect(Number(t[0].value.split(' ')[0])).toBeGreaterThanOrEqual(2)
  })

  it('renames Ports and Containers in a compose section', () => {
    applySnapshot(snapshot())
    setServicesSection({ compose: 'acme-shop' })
    expect(tiles(mount(StatTiles)).map((x) => x.label)).toEqual(['Ports', 'Containers', 'CPU', 'Memory'])
  })

  it('draws sparklines on CPU and Memory when wide', () => {
    applySnapshot(snapshot())
    const w = mount(StatTiles)
    const lines = w.findAllComponents(Sparkline)
    expect(lines).toHaveLength(2)
    expect(lines[0].classes()).toContain('text-accent')
    expect(lines[1].classes()).toContain('text-warm')
  })

  it('drops sparklines when narrow', () => {
    applySnapshot(snapshot())
    isNarrow.value = true
    expect(mount(StatTiles).findAllComponents(Sparkline)).toHaveLength(0)
  })
})

describe('Sparkline', () => {
  const pts = (series: number[]) =>
    mount(Sparkline, { props: { series } }).find('polyline').attributes('points')!.split(' ')

  it('draws a flat line with fewer than two points', () => {
    for (const series of [[], [5]]) {
      const ys = new Set(pts(series).map((p) => p.split(',')[1]))
      expect(ys.size).toBe(1)
    }
  })

  it('draws a flat line for a constant series', () => {
    const ys = new Set(pts([3, 3, 3]).map((p) => p.split(',')[1]))
    expect(ys.size).toBe(1)
  })

  it('maps larger values higher (smaller y) and spans the full width', () => {
    const p = pts([1, 2, 3]).map((s) => s.split(',').map(Number))
    expect(p).toHaveLength(3)
    expect(p[0][0]).toBe(0)
    expect(p[2][0]).toBe(78)
    expect(p[2][1]).toBeLessThan(p[0][1])
  })

  it('strokes with currentColor', () => {
    expect(mount(Sparkline, { props: { series: [1, 2] } }).find('polyline').attributes('stroke')).toBe('currentColor')
  })
})
