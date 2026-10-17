import { enableAutoUnmount, flushPromises, mount } from '@vue/test-utils'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { databasesApi } from './api'
import DatabasesPage from './DatabasesPage.vue'
import { resetDatabases, useDatabases } from './store'
import { applySnapshot, resetServices } from '../services/store/services'
import { useToasts } from '../../ui/store/toasts'
import { answer, useConfirm } from '../../ui/store/confirm'
import { snapshot } from '../../test/fixtures'
import type { ConnectionsView, RunOutput } from './types'

vi.mock('./api', () => ({
  databasesApi: {
    list: vi.fn(),
    history: vi.fn(),
    run: vi.fn(),
    cancel: vi.fn(),
    setReadOnly: vi.fn(),
    remove: vi.fn(),
    save: vi.fn(),
    parseUrl: vi.fn(),
    test: vi.fn(),
    clearHistory: vi.fn(),
  },
}))
const api = vi.mocked(databasesApi)
enableAutoUnmount(afterEach)

const markErrorLine = vi.fn()
const SqlEditorStub = {
  name: 'SqlEditor',
  props: ['modelValue', 'history'],
  emits: ['update:modelValue', 'run'],
  methods: { markErrorLine },
  template:
    '<textarea data-testid="editor" :value="modelValue" @input="$emit(\'update:modelValue\', $event.target.value)" @keydown.meta.enter="$emit(\'run\')" />',
}
const DialogStub = {
  name: 'ConnectionDialog',
  props: ['open', 'initial'],
  emits: ['close', 'saved'],
  template: '<div v-if="open" data-testid="dialog" />',
}

const view: ConnectionsView = {
  saved: [
    { id: 'a', name: 'Prod', env: 'production', color: null, host: 'h', port: 5432, user: 'u', database: 'd', sslMode: 'prefer', readOnly: true },
    { id: 'b', name: 'Local', env: null, color: '#0000ff', host: 'h', port: 5432, user: 'u', database: 'd', sslMode: 'prefer', readOnly: false },
  ],
  detected: [{ id: 'det1', containerId: 'c1', name: 'pg-run', project: 'shop', port: 5433, running: true }],
  dockerAvailable: true,
}
const output: RunOutput = { statements: [{ rows: 1, returnedColumns: true }], result: { columns: ['x'], rows: [['1']], truncated: false }, elapsedMs: 3, rolledBack: false }

beforeEach(() => {
  vi.resetAllMocks()
  localStorage.clear()
  resetDatabases()
  resetServices()
  useToasts().items.splice(0)
  api.list.mockResolvedValue(view)
  api.history.mockResolvedValue([])
})
afterEach(() => {
  vi.useRealTimers()
})

async function mountPage() {
  const w = mount(DatabasesPage, { global: { stubs: { SqlEditor: SqlEditorStub, ConnectionDialog: DialogStub } }, attachTo: document.body })
  await flushPromises()
  return w
}
const rowOf = (w: Awaited<ReturnType<typeof mountPage>>, name: string) =>
  w.findAll('[data-testid="saved-row"]').find((r) => r.text().includes(name))!

describe('DatabasesPage', () => {
  it('shows the empty state when nothing is saved or detected, and its button opens the dialog', async () => {
    api.list.mockResolvedValue({ saved: [], detected: [], dockerAvailable: true })
    const w = await mountPage()
    expect(w.find('h2').text()).toBe('Query a local Postgres')
    expect(w.text()).toContain('LsDev found no running Postgres')
    expect(w.find('[data-testid="editor"]').exists()).toBe(false)
    await w.findAll('button').find((b) => b.text().includes('Add connection…'))!.trigger('click')
    expect(w.find('[data-testid="dialog"]').exists()).toBe(true)
  })

  it('the header subtitle reads "Postgres connections"', async () => {
    const w = await mountPage()
    expect(w.text()).toContain('Postgres connections')
    expect(w.text()).not.toContain('Local Postgres instances')
  })

  it('switching connection empties the history before the new one loads', async () => {
    api.history.mockResolvedValue(['select from_local'])
    const w = await mountPage()
    await rowOf(w, 'Local').trigger('click')
    await flushPromises()
    expect(w.findComponent(SqlEditorStub).props('history')).toEqual(['select from_local'])
    let resolve!: (h: string[]) => void
    api.history.mockReturnValue(new Promise((r) => (resolve = r)))
    await rowOf(w, 'Prod').trigger('click')
    expect(w.findComponent(SqlEditorStub).props('history')).toEqual([])
    resolve(['select from_prod'])
    await flushPromises()
    expect(w.findComponent(SqlEditorStub).props('history')).toEqual(['select from_prod'])
  })

  it('Clear history asks confirmation, clears the active connection history and the list', async () => {
    api.history.mockResolvedValue(['select 1'])
    api.clearHistory.mockResolvedValue()
    const w = await mountPage()
    await rowOf(w, 'Local').trigger('click')
    await flushPromises()
    await w.find('[aria-label="Clear history"]').trigger('click')
    const req = useConfirm().request!
    expect(req.title).toBe('Clear the query history of Local?')
    expect(api.clearHistory).not.toHaveBeenCalled()
    answer({ ok: true, checked: false })
    await flushPromises()
    expect(api.clearHistory).toHaveBeenCalledWith('b')
    expect(w.findComponent(SqlEditorStub).props('history')).toEqual([])
  })

  it('Clear history cancelled keeps the history', async () => {
    api.history.mockResolvedValue(['select 1'])
    const w = await mountPage()
    await rowOf(w, 'Local').trigger('click')
    await flushPromises()
    await w.find('[aria-label="Clear history"]').trigger('click')
    answer({ ok: false, checked: false })
    await flushPromises()
    expect(api.clearHistory).not.toHaveBeenCalled()
    expect(w.findComponent(SqlEditorStub).props('history')).toEqual(['select 1'])
  })

  it('a failed Clear history shows an error toast', async () => {
    api.history.mockResolvedValue(['select 1'])
    api.clearHistory.mockRejectedValue(new Error('disk full'))
    const w = await mountPage()
    await rowOf(w, 'Local').trigger('click')
    await flushPromises()
    await w.find('[aria-label="Clear history"]').trigger('click')
    answer({ ok: true, checked: false })
    await flushPromises()
    expect(useToasts().items).toMatchObject([{ kind: 'error', message: 'disk full' }])
  })

  it('Clear history is disabled while the history is empty', async () => {
    const w = await mountPage()
    await rowOf(w, 'Local').trigger('click')
    await flushPromises()
    expect(w.find('[aria-label="Clear history"]').attributes('disabled')).toBeDefined()
  })

  it('forwards the Settings button as open-settings', async () => {
    const w = await mountPage()
    await w.find('[aria-label="Settings"]').trigger('click')
    expect(w.emitted('open-settings')).toHaveLength(1)
  })

  it('the header action opens the dialog for a new connection', async () => {
    const w = await mountPage()
    await w.findAll('button').find((b) => b.text() === 'Add connection')!.trigger('click')
    expect(w.findComponent(DialogStub).props()).toMatchObject({ open: true, initial: null })
  })

  it('shows the disabled placeholder editor while no connection is active', async () => {
    const w = await mountPage()
    expect(w.find('[data-testid="editor"]').exists()).toBe(false)
    expect(w.text()).toContain('-- Pick a connection to start writing SQL')
    expect(w.findAll('button').find((b) => b.text().startsWith('Run'))!.attributes('disabled')).toBeDefined()
  })

  it('selecting a connection loads its history and draft', async () => {
    localStorage.setItem('lsdev.databases.drafts', JSON.stringify({ b: 'select 1' }))
    resetDatabases()
    api.history.mockResolvedValue(['select 2'])
    const w = await mountPage()
    await rowOf(w, 'Local').trigger('click')
    await flushPromises()
    expect(api.history).toHaveBeenCalledWith('b')
    expect((w.find('[data-testid="editor"]').element as HTMLTextAreaElement).value).toBe('select 1')
    expect(w.findComponent(SqlEditorStub).props('history')).toEqual(['select 2'])
    expect(w.text()).toContain('Local')
  })

  it('typing updates the draft of the active connection', async () => {
    const w = await mountPage()
    await rowOf(w, 'Local').trigger('click')
    await w.find('[data-testid="editor"]').setValue('select now()')
    expect(useDatabases().state.drafts.b).toBe('select now()')
  })

  it('⌘↵ in the editor and the Run button both call run(id, draft)', async () => {
    api.run.mockResolvedValue(output)
    const w = await mountPage()
    await rowOf(w, 'Local').trigger('click')
    await w.find('[data-testid="editor"]').setValue('select 1')
    await w.find('[data-testid="editor"]').trigger('keydown', { key: 'Enter', metaKey: true })
    await flushPromises()
    expect(api.run).toHaveBeenCalledWith('b', 'select 1')
    await w.findAll('button').find((b) => b.text().startsWith('Run'))!.trigger('click')
    await flushPromises()
    expect(api.run).toHaveBeenCalledTimes(2)
    // History is reloaded after each run.
    expect(api.history.mock.calls.filter(([id]) => id === 'b').length).toBeGreaterThanOrEqual(3)
  })

  it('while running, Cancel replaces Run and calls cancel(id)', async () => {
    let resolve!: (o: RunOutput) => void
    api.run.mockReturnValue(new Promise((r) => (resolve = r)))
    api.cancel.mockResolvedValue()
    const w = await mountPage()
    await rowOf(w, 'Local').trigger('click')
    await w.find('[data-testid="editor"]').setValue('select pg_sleep(10)')
    await w.findAll('button').find((b) => b.text().startsWith('Run'))!.trigger('click')
    const cancel = w.findAll('button').find((b) => b.text() === 'Cancel')
    expect(cancel).toBeDefined()
    expect(w.findAll('button').find((b) => b.text().startsWith('Run'))).toBeUndefined()
    await cancel!.trigger('click')
    expect(api.cancel).toHaveBeenCalledWith('b')
    resolve(output)
    await flushPromises()
    expect(w.findAll('button').find((b) => b.text() === 'Cancel')).toBeUndefined()
  })

  it('the Read-only switch reflects the connection and calls setReadOnly(id, false)', async () => {
    api.setReadOnly.mockResolvedValue()
    const w = await mountPage()
    await rowOf(w, 'Prod').trigger('click')
    const sw = w.find('[data-testid="read-only"]')
    expect(sw.attributes('aria-checked')).toBe('true')
    expect(w.text()).toContain('production')
    api.list.mockResolvedValue({ ...view, saved: [{ ...view.saved[0], readOnly: false }, view.saved[1]] })
    await sw.trigger('click')
    await flushPromises()
    expect(api.setReadOnly).toHaveBeenCalledWith('a', false)
    expect(w.find('[data-testid="read-only"]').attributes('aria-checked')).toBe('false')
    expect(rowOf(w, 'Prod').find('[data-testid="lock"]').exists()).toBe(false)
  })

  it('a failed Read-only toggle shows an error toast', async () => {
    api.setReadOnly.mockRejectedValue(new Error('boom'))
    const w = await mountPage()
    await rowOf(w, 'Prod').trigger('click')
    await w.find('[data-testid="read-only"]').trigger('click')
    await flushPromises()
    expect(useToasts().items).toMatchObject([{ kind: 'error', message: 'boom' }])
  })

  it('the editor border uses the connection colour', async () => {
    const w = await mountPage()
    await rowOf(w, 'Local').trigger('click')
    expect(w.find('[data-testid="editor-frame"]').attributes('style')).toContain('border-left-color: rgb(0, 0, 255)')
  })

  it('marks the error line of a Postgres error with a position', async () => {
    api.run.mockRejectedValue({ kind: 'db', message: 'syntax error', code: '42601', hint: null, position: 8 })
    const w = await mountPage()
    await rowOf(w, 'Local').trigger('click')
    await w.find('[data-testid="editor"]').setValue('select\nfrom')
    await w.findAll('button').find((b) => b.text().startsWith('Run'))!.trigger('click')
    await flushPromises()
    expect(markErrorLine).toHaveBeenLastCalledWith(8)
  })

  it('edit and "Save as connection…" open the dialog with the matching draft', async () => {
    const w = await mountPage()
    await w.find('[aria-label="Menu Local"]').trigger('click')
    await w.find('[data-action="edit"]').trigger('click')
    expect(w.findComponent(DialogStub).props()).toMatchObject({ open: true, initial: { ...view.saved[1] } })
    w.findComponent(DialogStub).vm.$emit('close')
    await flushPromises()
    expect(w.find('[data-testid="dialog"]').exists()).toBe(false)
    await w.find('[aria-label="Menu pg-run"]').trigger('click')
    await w.find('[data-action="save-detected"]').trigger('click')
    expect(w.findComponent(DialogStub).props('initial')).toEqual({
      id: null,
      name: 'pg-run',
      env: null,
      color: null,
      host: '127.0.0.1',
      port: 5433,
      user: 'postgres',
      database: '',
      sslMode: 'disable',
      readOnly: false,
    })
  })

  it('refreshes when the Services snapshot changes, at most once per 5 s', async () => {
    vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout', 'Date'] })
    await mountPage()
    expect(api.list).toHaveBeenCalledTimes(1)
    const base = snapshot()
    applySnapshot({ ...base, takenAt: 1000 })
    await flushPromises()
    applySnapshot({ ...base, takenAt: 2000 })
    await flushPromises()
    applySnapshot({ ...base, takenAt: 3000 })
    await flushPromises()
    const afterBurst = api.list.mock.calls.length
    expect(afterBurst).toBeLessThanOrEqual(2)
    vi.advanceTimersByTime(5000)
    await flushPromises()
    expect(api.list.mock.calls.length).toBe(afterBurst + 1)
  })
})
