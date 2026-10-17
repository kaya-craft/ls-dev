import { beforeEach, describe, expect, it, vi } from 'vitest'

async function freshView() {
  vi.resetModules()
  return import('./view')
}

describe('view store', () => {
  beforeEach(() => localStorage.clear())

  it('toggles the sort through desc, asc, off', async () => {
    const { useView, toggleSort } = await freshView()
    toggleSort('memory')
    expect(useView().state.sort).toEqual({ key: 'memory', dir: 'desc' })
    toggleSort('memory')
    expect(useView().state.sort).toEqual({ key: 'memory', dir: 'asc' })
    toggleSort('memory')
    expect(useView().state.sort).toBeNull()
  })

  it('toggles collapsed projects', async () => {
    const { toggleCollapsed, isCollapsed } = await freshView()
    expect(isCollapsed('p')).toBe(false)
    toggleCollapsed('p')
    expect(isCollapsed('p')).toBe(true)
    toggleCollapsed('p')
    expect(isCollapsed('p')).toBe(false)
  })

  it('persists and reloads the view', async () => {
    const first = await freshView()
    first.toggleSort('cpu')
    first.toggleCollapsed('proj')
    expect(JSON.parse(localStorage.getItem('lsdev.view')!)).toEqual({
      sort: { key: 'cpu', dir: 'desc' },
      collapsed: ['proj'],
    })
    const second = await freshView()
    expect(second.useView().state.sort).toEqual({ key: 'cpu', dir: 'desc' })
    expect(second.isCollapsed('proj')).toBe(true)
  })

  it('falls back to defaults on corrupted data', async () => {
    localStorage.setItem('lsdev.view', '{not json')
    const a = await freshView()
    expect(a.useView().state.sort).toBeNull()
    expect(a.useView().state.collapsed).toEqual([])
    localStorage.setItem('lsdev.view', JSON.stringify({ sort: { key: 'bogus', dir: 'up' }, collapsed: 'x' }))
    const b = await freshView()
    expect(b.useView().state.sort).toBeNull()
    expect(b.useView().state.collapsed).toEqual([])
  })

  it('works when localStorage throws', async () => {
    const spy = vi.spyOn(Storage.prototype, 'getItem').mockImplementation(() => {
      throw new Error('blocked')
    })
    const set = vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => {
      throw new Error('blocked')
    })
    const { useView, toggleSort } = await freshView()
    expect(useView().state.sort).toBeNull()
    expect(() => toggleSort('cpu')).not.toThrow()
    spy.mockRestore()
    set.mockRestore()
  })

  it('resetView restores the defaults', async () => {
    const { useView, toggleSort, toggleCollapsed, resetView } = await freshView()
    toggleSort('cpu')
    toggleCollapsed('p')
    resetView()
    expect(useView().state.sort).toBeNull()
    expect(useView().state.collapsed).toEqual([])
  })
})
