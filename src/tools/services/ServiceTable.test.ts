import { beforeEach, describe, expect, it, vi } from 'vitest'
import { mount } from '@vue/test-utils'
import ServiceTable from './ServiceTable.vue'
import { applySnapshot, resetServices, setFilter, useServices } from './store/services'
import { resetView, useView } from './store/view'
import { isNarrow } from '../../shell/viewport'
import { tableCompact } from './compact'
import { resetNavigation, setServicesSection } from '../../shell/navigation'
import { openBrowser } from './actions'
import { ctr, proc, snapshot } from '../../test/fixtures'

vi.mock('./api', () => ({ api: {} }))
vi.mock('./actions', () => ({ openBrowser: vi.fn() }))

describe('ServiceTable', () => {
  beforeEach(() => {
    resetServices()
    resetView()
    resetNavigation()
    tableCompact.value = false
    isNarrow.value = false
    vi.mocked(openBrowser).mockClear()
  })

  it('renders sections in order: dev servers, compose projects, standalone', () => {
    applySnapshot(snapshot())
    const titles = mount(ServiceTable).findAll('h2').map((h) => h.text())
    expect(titles[0]).toContain('Dev servers')
    expect(titles[1]).toContain('acme-shop')
    expect(titles[2]).toContain('Standalone containers')
  })

  it('shows the worktree branch as a badge', () => {
    applySnapshot(snapshot())
    const badge = mount(ServiceTable).find('[data-testid="worktree-badge"]')
    expect(badge.text()).toBe('feat/login')
  })

  it('marks responding and silent ports', () => {
    applySnapshot(snapshot())
    const wrapper = mount(ServiceTable)
    expect(wrapper.findAll('[aria-label="responding"]').length).toBeGreaterThan(0)
    expect(wrapper.findAll('[aria-label="not responding"]')).toHaveLength(1)
  })

  describe('section blocks', () => {
    const titles = () => mount(ServiceTable).findAll('h2').map((h) => h.text())
    beforeEach(() => applySnapshot(snapshot()))

    it('all: every block, with empty-state lines', () => {
      setServicesSection('all')
      expect(titles()).toHaveLength(3)
    })
    it('compose: only that project group', () => {
      setServicesSection({ compose: 'acme-shop' })
      const t = titles()
      expect(t).toHaveLength(1)
      expect(t[0]).toContain('acme-shop')
      const text = mount(ServiceTable).text()
      expect(text).not.toContain('Nothing listening')
      expect(text).not.toContain('Standalone containers')
    })
    it('dev: only Dev servers', () => {
      setServicesSection('dev')
      const t = titles()
      expect(t).toHaveLength(1)
      expect(t[0]).toContain('Dev servers')
    })
    it('standalone: only Standalone containers', () => {
      setServicesSection('standalone')
      const t = titles()
      expect(t).toHaveLength(1)
      expect(t[0]).toContain('Standalone containers')
    })
  })

  describe('ports column', () => {
    const threePorts = [
      { port: 3000, responsive: true },
      { port: 3001, responsive: false },
      { port: 3002, responsive: null },
    ]
    function devRow(overrides: Parameters<typeof proc>[0]) {
      applySnapshot({ ...snapshot(), processes: [proc(overrides)] })
      return mount(ServiceTable).find('section[aria-label="Dev servers"]').find('[role="row"]')
    }

    it('with three ports shows the first one then a +2 chip listing every port', () => {
      const row = devRow({ ports: threePorts })
      const cell = row.find('[data-testid="ports"]')
      expect(cell.classes()).toContain('flex-nowrap')
      expect(cell.classes()).not.toContain('flex-wrap')
      expect(cell.text()).toContain(':3000')
      expect(cell.text()).not.toContain(':3001')
      const more = cell.find('[data-testid="ports-more"]')
      expect(more.text()).toBe('+2')
      expect(more.classes()).toContain('flex-none')
      expect(more.attributes('title')).toBe(':3000, :3001, :3002')
    })

    it('compact: two ports show the first one then +1', () => {
      tableCompact.value = true
      const cell = devRow({ ports: threePorts.slice(0, 2) }).find('[data-testid="ports"]')
      tableCompact.value = false
      expect(cell.text()).toContain(':3000')
      expect(cell.text()).not.toContain(':3001')
      expect(cell.find('[data-testid="ports-more"]').text()).toBe('+1')
    })

    it('keeps the visible responding ports clickable', async () => {
      const row = devRow({ ports: threePorts })
      await row.find('button[aria-label="Open :3000 in browser"]').trigger('click')
      expect(openBrowser).toHaveBeenCalledWith(3000)
    })

    it('shows both ports and no chip for two ports', () => {
      const row = devRow({ ports: threePorts.slice(0, 2) })
      expect(row.find('[data-testid="ports-more"]').exists()).toBe(false)
      expect(row.find('[data-testid="ports"]').text()).toContain(':3001')
    })

    it('uses a 128 px ports column, also below 960 px', () => {
      expect(devRow({ ports: threePorts }).classes().join(' ')).toContain('grid-cols-[128px_')
      isNarrow.value = true
      expect(devRow({ ports: threePorts }).classes().join(' ')).toContain('grid-cols-[128px_')
    })
  })

  it('shows a dash and no meter for a running service without cpu / memory figures', () => {
    applySnapshot({ ...snapshot(), compose: [], standalone: [ctr({ state: 'running', cpuPercent: null, memBytes: null })] })
    const row = mount(ServiceTable).find('section[aria-label="Standalone containers"]').find('[role="row"]')
    expect(row.find('[role="meter"]').exists()).toBe(false)
    expect(row.text()).toContain('—')
  })

  it('shows a banner instead of containers when Docker is unreachable', () => {
    applySnapshot({ ...snapshot(), dockerAvailable: false, compose: [], standalone: [] })
    const wrapper = mount(ServiceTable)
    expect(wrapper.find('[data-testid="docker-banner"]').exists()).toBe(true)
    expect(wrapper.text()).not.toContain('Standalone containers')
  })

  it('selects a row on click', async () => {
    applySnapshot(snapshot())
    const wrapper = mount(ServiceTable)
    await wrapper.findAll('[role="row"]')[1].trigger('click')
    expect(useServices().state.selectedId).toBe(snapshot().processes[0].id)
  })

  function processNames(wrapper: ReturnType<typeof mount>) {
    return wrapper
      .find('section[aria-label="Dev servers"]')
      .findAll('[role="row"]')
      .map((r) => r.text())
  }

  it('orders process rows by CPU on header click, desc then asc', async () => {
    const snap = snapshot()
    snap.processes = [
      proc({ id: 'proc:1:/a', project: 'low-cpu', cpuPercent: 1 }),
      proc({ id: 'proc:2:/b', project: 'high-cpu', cpuPercent: 80 }),
      proc({ id: 'proc:3:/c', project: 'mid-cpu', cpuPercent: 20 }),
    ]
    applySnapshot(snap)
    const wrapper = mount(ServiceTable)
    expect(wrapper.find('[data-sort="cpu"]').element.closest('[aria-sort]')?.getAttribute('aria-sort')).toBe('none')
    await wrapper.find('[data-sort="cpu"]').trigger('click')
    expect(processNames(wrapper).map((t) => t.match(/[a-z]+-cpu/)![0])).toEqual(['high-cpu', 'mid-cpu', 'low-cpu'])
    expect(wrapper.find('[data-sort="cpu"]').element.closest('[aria-sort]')?.getAttribute('aria-sort')).toBe('descending')
    expect(wrapper.find('[data-sort="cpu"]').text()).toContain('↓')
    await wrapper.find('[data-sort="cpu"]').trigger('click')
    expect(processNames(wrapper).map((t) => t.match(/[a-z]+-cpu/)![0])).toEqual(['low-cpu', 'mid-cpu', 'high-cpu'])
    expect(wrapper.find('[data-sort="cpu"]').element.closest('[aria-sort]')?.getAttribute('aria-sort')).toBe('ascending')
    expect(wrapper.find('[data-sort="cpu"]').text()).toContain('↑')
  })

  it('collapses a Compose group and shows a summary', async () => {
    applySnapshot(snapshot())
    const wrapper = mount(ServiceTable, {
      slots: { 'group-actions': '<button>Stop all</button>' },
    })
    const group = () => wrapper.find('section[aria-label="acme-shop"]')
    expect(group().findAll('[role="row"]')).toHaveLength(2)
    const toggle = group().find('button[aria-expanded]')
    expect(toggle.attributes('aria-expanded')).toBe('true')
    await toggle.trigger('click')
    expect(group().findAll('[role="row"]')).toHaveLength(0)
    expect(group().find('button[aria-expanded]').attributes('aria-expanded')).toBe('false')
    expect(group().find('[data-testid="group-summary"]').text()).toBe('2 containers · 2 running · 0.8% · 160 MB')
    expect(group().text()).toContain('Stop all')
    await group().find('button[aria-expanded]').trigger('click')
    expect(group().findAll('[role="row"]')).toHaveLength(2)
  })

  it('summarises a group whose stats are all null with dashes', async () => {
    const snap = snapshot()
    for (const c of snap.compose[0].containers) {
      c.cpuPercent = null
      c.memBytes = null
    }
    applySnapshot(snap)
    const wrapper = mount(ServiceTable)
    await wrapper.find('section[aria-label="acme-shop"] button[aria-expanded]').trigger('click')
    expect(wrapper.find('[data-testid="group-summary"]').text()).toBe('2 containers · 2 running · — · —')
  })

  it('shows matching rows of a collapsed group while filtering', async () => {
    applySnapshot(snapshot())
    const wrapper = mount(ServiceTable)
    await wrapper.find('section[aria-label="acme-shop"] button[aria-expanded]').trigger('click')
    expect(wrapper.findAll('section[aria-label="acme-shop"] [role="row"]')).toHaveLength(0)
    setFilter('redis')
    await wrapper.vm.$nextTick()
    const rows = wrapper.findAll('section[aria-label="acme-shop"] [role="row"]')
    expect(rows).toHaveLength(1)
    expect(rows[0].text()).toContain('redis')
    setFilter('')
    await wrapper.vm.$nextTick()
    expect(wrapper.findAll('section[aria-label="acme-shop"] [role="row"]')).toHaveLength(0)
  })

  it('keeps a container selected inside a collapsed group', async () => {
    applySnapshot(snapshot())
    useServices().state.selectedId = 'docker:aaa'
    const wrapper = mount(ServiceTable)
    await wrapper.find('section[aria-label="acme-shop"] button[aria-expanded]').trigger('click')
    expect(useServices().selected.value?.id).toBe('docker:aaa')
  })

  it('uses the singular for a one-container group', async () => {
    const snap = snapshot()
    snap.compose[0].containers = [snap.compose[0].containers[0]]
    applySnapshot(snap)
    const wrapper = mount(ServiceTable)
    await wrapper.find('section[aria-label="acme-shop"] button[aria-expanded]').trigger('click')
    expect(wrapper.find('[data-testid="group-summary"]').text()).toBe('1 container · 1 running · 0.4% · 80 MB')
  })

  it('disables the group toggle while filtering and leaves the stored list alone', async () => {
    applySnapshot(snapshot())
    const wrapper = mount(ServiceTable)
    setFilter('redis')
    await wrapper.vm.$nextTick()
    const toggle = wrapper.find('section[aria-label="acme-shop"] button[aria-expanded]')
    expect(toggle.attributes('disabled')).toBeDefined()
    await toggle.trigger('click')
    expect(useView().state.collapsed).toEqual([])
    setFilter('')
    await wrapper.vm.$nextTick()
    expect(wrapper.find('section[aria-label="acme-shop"] button[aria-expanded]').attributes('disabled')).toBeUndefined()
  })

  describe('redesigned table', () => {
    const headers = (w: ReturnType<typeof mount>) =>
      w.findAll('[role="columnheader"]').map((h) => h.text())
    const dev = (w: ReturnType<typeof mount>) => w.find('section[aria-label="Dev servers"]')

    it('has the columns Ports, Service, Uptime, CPU, Memory', () => {
      applySnapshot(snapshot())
      const w = mount(ServiceTable)
      expect(headers(w)).toEqual(['Ports', 'Service', 'Uptime', 'CPU', 'Memory'])
      expect(w.text()).not.toContain('Command / image')
      expect(w.text()).not.toContain('PID')
    })

    it('renders the pastille, the split name and the sub-line of a process row', () => {
      applySnapshot(snapshot())
      const row = dev(mount(ServiceTable)).findAll('[role="row"]')[0]
      expect(row.find('[aria-hidden="true"].rounded-full').text()).toBe('N')
      expect(row.find('[data-testid="name-prefix"]').text()).toBe('shop/packages/')
      expect(row.find('[data-testid="name-prefix"]').classes()).toContain('text-ink-3')
      expect(row.find('[data-testid="name-last"]').text()).toBe('backend')
      expect(row.find('[data-testid="name-last"]').classes()).toContain('font-medium')
      expect(row.find('[data-testid="subline"]').text()).toBe('pnpm --filter @acme/backend dev · 29414')
    })

    it('shows a managed badge', () => {
      const snap = snapshot()
      snap.processes[0].managed = true
      applySnapshot(snap)
      expect(dev(mount(ServiceTable)).find('[data-testid="managed-badge"]').text()).toBe('managed')
    })

    it('opens a responding port in the browser and leaves a silent port inert', async () => {
      applySnapshot(snapshot())
      const w = mount(ServiceTable)
      const buttons = dev(w).findAll('button')
      const link = buttons.find((b) => b.text() === ':4000')!
      expect(link.classes()).toContain('text-accent')
      await link.trigger('click')
      expect(openBrowser).toHaveBeenCalledWith(4000)
      expect(useServices().state.selectedId).toBeNull()
      expect(buttons.some((b) => b.text() === ':3000')).toBe(false)
      const silent = dev(w).findAll('span').find((s) => s.text() === ':3000')!
      expect(silent.classes()).toContain('text-ink-3')
    })

    it('renders a meter and a mono value for CPU and Memory, and a dash without meter when stopped', () => {
      applySnapshot(snapshot())
      const w = mount(ServiceTable)
      const row = dev(w).findAll('[role="row"]')[0]
      expect(row.findAll('[role="meter"]')).toHaveLength(2)
      expect(row.text()).toContain('1.5%')
      expect(row.text()).toContain('200 MB')
      const stopped = w.find('section[aria-label="Standalone containers"] [role="row"]')
      expect(stopped.findAll('[role="meter"]')).toHaveLength(0)
      expect(stopped.text()).toContain('—')
    })

    it('scales meters over the rows displayed', () => {
      const snap = snapshot()
      snap.processes = [
        proc({ id: 'proc:1:/a', project: 'a', cpuPercent: 20 }),
        proc({ id: 'proc:2:/b', project: 'b', cpuPercent: 5 }),
      ]
      applySnapshot(snap)
      const rows = dev(mount(ServiceTable)).findAll('[role="row"]')
      const width = (i: number) => (rows[i].find('[role="meter"][aria-label="CPU"] [data-fill]').element as HTMLElement).style.width
      expect(width(0)).toBe('100%')
      expect(width(1)).toBe('25%')
    })

    it('sinks stopped rows below running rows whatever the sort', async () => {
      const snap = snapshot()
      snap.compose[0].containers = [
        ctr({ id: 'docker:s', name: 'stopped-one', state: 'exited', status: 'Exited (0) 1h ago', uptimeSec: null, cpuPercent: null, memBytes: null }),
        ctr({ id: 'docker:r1', name: 'running-low', cpuPercent: 1 }),
        ctr({ id: 'docker:r2', name: 'running-high', cpuPercent: 9 }),
      ]
      applySnapshot(snap)
      const w = mount(ServiceTable)
      const order = () =>
        w.findAll('section[aria-label="acme-shop"] [role="row"]').map((r) => r.text().match(/(stopped-one|running-low|running-high)/)![0])
      expect(order()).toEqual(['running-low', 'running-high', 'stopped-one'])
      await w.find('[data-sort="cpu"]').trigger('click')
      expect(order()).toEqual(['running-high', 'running-low', 'stopped-one'])
      await w.find('[data-sort="cpu"]').trigger('click')
      expect(order()).toEqual(['running-low', 'running-high', 'stopped-one'])
    })

    it('titles the sections with their counts', () => {
      applySnapshot(snapshot())
      const titles = mount(ServiceTable).findAll('h2').map((h) => h.text())
      expect(titles[0]).toBe('Dev servers · 2')
      expect(titles[2]).toBe('Standalone containers · 0 running')
    })

    it('shows the Compose header with chevron, name, mini pastilles and actions', () => {
      applySnapshot(snapshot())
      const w = mount(ServiceTable, { slots: { 'group-actions': '<button>Start all</button><button>Stop all</button>' } })
      const head = w.find('section[aria-label="acme-shop"] h2')
      expect(head.find('svg').exists()).toBe(true)
      expect(head.text()).toContain('acme-shop')
      expect(head.findAll('[data-testid="mini-pastille"]').map((m) => m.text())).toEqual(['Pg', 'Rd'])
      expect(head.text()).toContain('Start all')
      expect(head.text()).toContain('Stop all')
      expect(head.find('[data-testid="group-summary"]').exists()).toBe(false)
    })

    it('narrow: no Uptime header, uptime appended to the sub-line', () => {
      isNarrow.value = true
      applySnapshot(snapshot())
      const w = mount(ServiceTable)
      expect(headers(w)).toEqual(['Ports', 'Service', 'CPU', 'Memory'])
      expect(w.find('[data-sort="uptime"]').exists()).toBe(false)
      const row = dev(w).findAll('[role="row"]')[0]
      expect(row.find('[data-testid="subline"]').text()).toBe('pnpm --filter @acme/backend dev · 29414 · 1h0m')
    })

    it('narrow: short collapsed summary and a … menu holding the group actions', async () => {
      isNarrow.value = true
      applySnapshot(snapshot())
      const w = mount(ServiceTable, { slots: { 'group-actions': '<button>Start all</button><button>Stop all</button>' } })
      const group = () => w.find('section[aria-label="acme-shop"]')
      await group().find('button[aria-expanded]').trigger('click')
      expect(group().find('[data-testid="group-summary"]').text()).toBe('2/2 · 0.8% · 160 MB')
      expect(group().text()).not.toContain('Start all')
      const menu = group().find('button[aria-label="Group actions"]')
      expect(menu.text()).toBe('…')
      await menu.trigger('click')
      expect(group().text()).toContain('Start all')
      expect(group().text()).toContain('Stop all')
    })

    describe('narrow … menu dismissal', () => {
      async function openMenu() {
        isNarrow.value = true
        applySnapshot(snapshot())
        useServices().state.selectedId = 'docker:aaa'
        const w = mount(ServiceTable, {
          attachTo: document.body,
          slots: { 'group-actions': '<button>Start all</button><button>Stop all</button>' },
        })
        await w.find('button[aria-label="Group actions"]').trigger('click')
        expect(w.text()).toContain('Start all')
        return w
      }

      it('closes on Escape without letting it reach the global handler', async () => {
        const w = await openMenu()
        const global = vi.fn()
        window.addEventListener('keydown', global)
        document.body.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }))
        await w.vm.$nextTick()
        window.removeEventListener('keydown', global)
        expect(w.text()).not.toContain('Start all')
        expect(global).not.toHaveBeenCalled()
        expect(useServices().state.selectedId).toBe('docker:aaa')
        w.unmount()
      })

      it('closes on an outside click', async () => {
        const w = await openMenu()
        document.body.dispatchEvent(new MouseEvent('click', { bubbles: true }))
        await w.vm.$nextTick()
        expect(w.text()).not.toContain('Start all')
        w.unmount()
      })

      it('stays open when the pointer moves from the button onto the popover', async () => {
        const w = await openMenu()
        const popover = w.find('[data-group-menu] > span')
        await popover.trigger('mouseleave')
        expect(w.text()).toContain('Start all')
        await w.find('[data-group-menu]').trigger('mouseleave')
        expect(w.text()).not.toContain('Start all')
        w.unmount()
      })

      it('is a plain disclosure: aria-expanded, no menu role', async () => {
        const w = await openMenu()
        const btn = w.find('button[aria-label="Group actions"]')
        expect(btn.attributes('aria-expanded')).toBe('true')
        expect(btn.attributes('aria-haspopup')).toBeUndefined()
        expect(w.find('[role="menu"]').exists()).toBe(false)
        w.unmount()
      })
    })
  })

  describe('compact mode (observed table width)', () => {
    const observers: Array<(entries: Array<{ contentRect: { width: number } }>) => void> = []
    beforeEach(() => {
      observers.length = 0
      vi.stubGlobal(
        'ResizeObserver',
        class {
          constructor(cb: (entries: Array<{ contentRect: { width: number } }>) => void) {
            observers.push(cb)
          }
          observe() {}
          disconnect() {}
        },
      )
    })

    it('uses the compact grid, no uptime column and no meters below 640 px', async () => {
      applySnapshot(snapshot())
      const wrapper = mount(ServiceTable)
      observers[0]([{ contentRect: { width: 600 } }])
      await wrapper.vm.$nextTick()
      const header = wrapper.find('[role="row"]')
      expect(header.classes().join(' ')).toContain('grid-cols-[96px_minmax(0,1fr)_64px_80px]')
      expect(wrapper.text()).not.toContain('Uptime')
      expect(wrapper.find('[data-sort="uptime"]').exists()).toBe(false)
      expect(wrapper.findAll('[role="row"]')[1].classes().join(' ')).toContain('64px_80px')
      observers[0]([{ contentRect: { width: 900 } }])
      await wrapper.vm.$nextTick()
      expect(wrapper.find('[data-sort="uptime"]').exists()).toBe(true)
      wrapper.unmount()
    })
  })

  it('keeps a minimum width on the name when a worktree badge is present', () => {
    applySnapshot(snapshot())
    const wrapper = mount(ServiceTable)
    const row = wrapper.findAll('[role="row"]').find((r) => r.find('[data-testid="worktree-badge"]').exists())!
    expect(row.find('[data-testid="name"]').classes()).toContain('min-w-[80px]')
    expect(row.find('[data-testid="worktree-badge"]').classes()).toContain('truncate')
  })
})
