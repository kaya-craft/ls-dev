import { beforeEach, describe, expect, it } from 'vitest'
import { applyInitialSnapshot, applySnapshot, resetServices, select, setFilter, useServices } from './services'
import { snapshot } from '../../../test/fixtures'
import { resetView, toggleSort } from './view'
import { resetNavigation, useNavigation } from '../../../shell/navigation'

describe('services store', () => {
  beforeEach(() => {
    resetServices()
    resetView()
  })

  it('is loading until a real snapshot arrives', () => {
    const { loading } = useServices()
    expect(loading.value).toBe(true)
    applySnapshot({ ...snapshot(), takenAt: 0 })
    expect(loading.value).toBe(true)
    applySnapshot(snapshot())
    expect(loading.value).toBe(false)
  })

  it('filters processes by port, project and script', () => {
    applySnapshot(snapshot())
    const { sections } = useServices()
    setFilter('4000')
    expect(sections.value.processes.map((p) => p.project)).toEqual(['shop/packages/backend'])
    setFilter('dev:app')
    expect(sections.value.processes.map((p) => p.ports[0].port)).toEqual([3000])
    setFilter('feat/login')
    expect(sections.value.processes).toHaveLength(1)
  })

  it('filters containers by image and keeps a group whose project matches', () => {
    applySnapshot(snapshot())
    const { sections } = useServices()
    setFilter('redis')
    expect(sections.value.compose[0].containers.map((c) => c.name)).toEqual(['acme-shop-redis-1'])
    setFilter('acme-shop')
    expect(sections.value.compose[0].containers).toHaveLength(2)
    setFilter('mysql')
    expect(sections.value.compose).toHaveLength(0)
    expect(sections.value.standalone.map((c) => c.name)).toEqual(['legacy-mysql'])
  })

  it('keeps the selection while the service exists and drops it when it disappears', () => {
    applySnapshot(snapshot())
    const { selected } = useServices()
    select('docker:bbb')
    expect(selected.value?.id).toBe('docker:bbb')
    const next = snapshot()
    next.compose[0].containers.pop()
    applySnapshot(next)
    expect(selected.value).toBeNull()
  })

  it('clears the error when a snapshot arrives', () => {
    const { state } = useServices()
    state.error = 'boom'
    applySnapshot(snapshot())
    expect(state.error).toBeNull()
  })

  it('ignores an initial snapshot older than the one already held', () => {
    const { state } = useServices()
    applySnapshot({ ...snapshot(), takenAt: 2000 })
    applyInitialSnapshot({ ...snapshot(), takenAt: 1000 })
    expect(state.snapshot?.takenAt).toBe(2000)
    applyInitialSnapshot({ ...snapshot(), takenAt: 3000 })
    expect(state.snapshot?.takenAt).toBe(3000)
  })

  it('applies the sort inside each section but keeps group order', () => {
    applySnapshot(snapshot())
    toggleSort('cpu')
    const { sections } = useServices()
    expect(sections.value.processes.map((p) => p.cpuPercent)).toEqual([1.5, 1.5])
    expect(sections.value.compose[0].containers.map((c) => c.name)).toEqual([
      'acme-shop-postgres-1',
      'acme-shop-redis-1',
    ])
    const s = snapshot()
    s.compose[0].containers[1].cpuPercent = 9
    applySnapshot(s)
    expect(useServices().sections.value.compose[0].containers.map((c) => c.cpuPercent)).toEqual([9, 0.4])
  })

  it('filters sections by the sidebar selection without dropping the selection', async () => {
    const { setServicesSection, resetNavigation } = await import('../../../shell/navigation')
    resetNavigation()
    applySnapshot(snapshot())
    const { sections, selected } = useServices()
    select('docker:bbb')
    setServicesSection('dev')
    expect(sections.value.compose).toHaveLength(0)
    expect(sections.value.standalone).toHaveLength(0)
    expect(sections.value.processes.length).toBeGreaterThan(0)
    expect(selected.value?.id).toBe('docker:bbb')
    setServicesSection({ compose: 'acme-shop' })
    expect(sections.value.processes).toHaveLength(0)
    expect(sections.value.compose.map((g) => g.project)).toEqual(['acme-shop'])
    setServicesSection('all')
  })

  it('counts listening ports of processes and running containers', () => {
    applySnapshot(snapshot())
    expect(useServices().listeningCount.value).toBe(4)
  })

  it('treats an unknown compose section as all, without changing the stored value', () => {
    resetNavigation()
    applySnapshot(snapshot())
    useNavigation().state.servicesSection = { compose: 'gone' }
    const { sections } = useServices()
    expect(sections.value.processes).toHaveLength(2)
    expect(sections.value.standalone).toHaveLength(1)
    expect(sections.value.compose).toHaveLength(1)
    expect(useNavigation().state.servicesSection).toEqual({ compose: 'gone' })
    resetNavigation()
  })

  it('records a bounded history per section and exposes stats for the effective section', () => {
    resetNavigation()
    // Fixture base: proc 1.5 + two containers at 0.4 (compose total 0.8); the first process cpu is overridden.
    // all = cpu + 1.5 + 0.8, so cpu 1/2/3 gives 3.3/4.3/5.3.
    let at = 1_800_000_000_000
    const withCpu = (cpu: number) => {
      const s = { ...snapshot(), takenAt: at++ }
      s.processes[0].cpuPercent = cpu
      return s
    }
    applySnapshot(withCpu(1))
    applySnapshot(withCpu(2))
    applySnapshot(withCpu(3))
    const { history, stats } = useServices()
    expect(history.value.cpu).toEqual([3.3, 4.3, 5.3].map((n) => expect.closeTo(n, 5)))
    expect(stats.value.isCompose).toBe(false)
    useNavigation().state.servicesSection = { compose: 'acme-shop' }
    expect(history.value.cpu).toHaveLength(3)
    expect(history.value.cpu[0]).toBeCloseTo(0.8, 5)
    expect(history.value.cpu).toEqual([0.8, 0.8, 0.8].map((n) => expect.closeTo(n, 5)))
    expect(stats.value.isCompose).toBe(true)
    expect(stats.value.running).toBe(2)
    useNavigation().state.servicesSection = 'all'
    expect(history.value.cpu).toEqual([3.3, 4.3, 5.3].map((n) => expect.closeTo(n, 5)))
    expect(stats.value.isCompose).toBe(false)
    useNavigation().state.servicesSection = { compose: 'acme-shop' }
    setFilter('redis')
    expect(stats.value.running).toBe(2)
    resetNavigation()
  })

  it('prunes the history of a compose project absent from the next snapshot', () => {
    resetNavigation()
    applySnapshot(snapshot())
    const { state } = useServices()
    expect(Object.keys(state.history).sort()).toEqual(['all', 'compose:acme-shop', 'dev', 'standalone'])
    const next = { ...snapshot(), takenAt: snapshot().takenAt + 1 }
    next.compose = []
    applySnapshot(next)
    expect(Object.keys(state.history).sort()).toEqual(['all', 'dev', 'standalone'])
  })

  it('records no history for the startup placeholder snapshot', () => {
    applySnapshot({ ...snapshot(), takenAt: 0 })
    expect(useServices().state.history).toEqual({})
  })

  it('records one point when the same snapshot time arrives twice', () => {
    applySnapshot(snapshot())
    applySnapshot(snapshot())
    expect(useServices().history.value.cpu).toHaveLength(1)
    applySnapshot({ ...snapshot(), takenAt: snapshot().takenAt + 1 })
    expect(useServices().history.value.cpu).toHaveLength(2)
  })
})
