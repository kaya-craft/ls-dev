import { mount, flushPromises } from '@vue/test-utils'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { databasesApi } from './api'
import ConnectionList from './ConnectionList.vue'
import { resetDatabases, useDatabases } from './store'
import { api as servicesApi } from '../services/api'
import { answer, useConfirm } from '../../ui/store/confirm'
import type { ConnectionsView } from './types'

vi.mock('./api', () => ({ databasesApi: { list: vi.fn(), remove: vi.fn() } }))
vi.mock('../services/api', () => ({ api: { containerAction: vi.fn() } }))

const view: ConnectionsView = {
  saved: [
    { id: 'a', name: 'Prod', env: 'production', color: null, host: 'h', port: 5432, user: 'u', database: 'd', sslMode: 'prefer', readOnly: true },
    { id: 'b', name: 'Local', env: null, color: '#00f', host: 'h', port: 5432, user: 'u', database: 'd', sslMode: 'prefer', readOnly: false },
  ],
  detected: [
    { id: 'det1', containerId: 'c1', name: 'pg-run', project: 'shop', port: 5433, running: true },
    { id: 'det2', containerId: 'c2', name: 'pg-stop', project: null, port: null, running: false },
  ],
  dockerAvailable: true,
}

beforeEach(() => {
  vi.resetAllMocks()
  localStorage.clear()
  resetDatabases()
  vi.mocked(databasesApi.list).mockResolvedValue(view)
})

async function mountList() {
  await useDatabases().refresh()
  return mount(ConnectionList, { attachTo: document.body })
}

describe('ConnectionList', () => {
  it('renders saved and detected, with lock icon on read-only', async () => {
    const w = await mountList()
    const rows = w.findAll('[data-testid="saved-row"]')
    expect(rows).toHaveLength(2)
    expect(rows[0].text()).toContain('Prod')
    expect(rows[0].text()).toContain('production')
    expect(rows[0].find('[data-testid="lock"]').exists()).toBe(true)
    expect(rows[1].find('[data-testid="lock"]').exists()).toBe(false)
    expect(w.findAll('[data-testid="detected"]')).toHaveLength(2)
    expect(w.text()).toContain('shop')
    expect(w.text()).toContain(':5433')
  })

  it('shows the empty box when nothing is saved', async () => {
    vi.mocked(databasesApi.list).mockResolvedValue({ ...view, saved: [] })
    const w = await mountList()
    expect(w.text()).toContain('No saved connections')
  })

  it('selects a row on click', async () => {
    const w = await mountList()
    await w.findAll('[data-testid="saved-row"]')[1].trigger('click')
    expect(useDatabases().state.activeId).toBe('b')
  })

  it('selects a running detected container on click, not a stopped one', async () => {
    const w = await mountList()
    const [running, stopped] = w.findAll('[data-testid="detected"]')
    await stopped.trigger('click')
    expect(useDatabases().state.activeId).toBe(null)
    await running.trigger('click')
    expect(useDatabases().state.activeId).toBe('det1')
    expect(running.attributes('aria-current')).toBe('true')
  })

  it('Start does not select the stopped container', async () => {
    vi.mocked(servicesApi.containerAction).mockResolvedValue(undefined)
    const w = await mountList()
    await w.findAll('[data-testid="detected"]')[1].find('[data-action="start"]').trigger('click')
    expect(useDatabases().state.activeId).toBe(null)
  })

  it('Start calls containerAction with the docker: id then refreshes', async () => {
    vi.mocked(servicesApi.containerAction).mockResolvedValue()
    const w = await mountList()
    const stopped = w.findAll('[data-testid="detected"]')[1]
    await stopped.find('button[data-action="start"]').trigger('click')
    await flushPromises()
    expect(servicesApi.containerAction).toHaveBeenCalledWith('docker:c2', 'start')
    expect(databasesApi.list).toHaveBeenCalledTimes(2)
  })

  it('Edit emits the saved connection', async () => {
    const w = await mountList()
    await w.findAll('[data-testid="saved-row"]')[0].find('button[aria-label^="Menu"]').trigger('click')
    await w.find('button[data-action="edit"]').trigger('click')
    expect(w.emitted('edit')![0][0]).toMatchObject({ id: 'a' })
  })

  it('Delete asks confirmation, then removes and refreshes', async () => {
    vi.mocked(databasesApi.remove).mockResolvedValue()
    const w = await mountList()
    await w.findAll('[data-testid="saved-row"]')[0].find('button[aria-label^="Menu"]').trigger('click')
    await w.find('button[data-action="delete"]').trigger('click')
    const req = useConfirm().request!
    expect(req.title).toBe('Delete connection Prod?')
    expect(req.body).toBe('The saved password is removed from the keychain.')
    expect(req.danger).toBe(true)
    expect(databasesApi.remove).not.toHaveBeenCalled()
    answer({ ok: true, checked: false })
    await flushPromises()
    expect(databasesApi.remove).toHaveBeenCalledWith('a')
    expect(databasesApi.list).toHaveBeenCalledTimes(2)
  })

  it('Delete cancelled does nothing', async () => {
    const w = await mountList()
    await w.findAll('[data-testid="saved-row"]')[0].find('button[aria-label^="Menu"]').trigger('click')
    await w.find('button[data-action="delete"]').trigger('click')
    answer({ ok: false, checked: false })
    await flushPromises()
    expect(databasesApi.remove).not.toHaveBeenCalled()
  })

  it('Save as connection emits the detected one', async () => {
    const w = await mountList()
    await w.findAll('[data-testid="detected"]')[0].find('button[aria-label^="Menu"]').trigger('click')
    await w.find('button[data-action="save-detected"]').trigger('click')
    expect(w.emitted('save-detected')![0][0]).toMatchObject({ id: 'det1' })
  })
})
