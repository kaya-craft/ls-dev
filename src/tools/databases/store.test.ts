import { beforeEach, describe, expect, it, vi } from 'vitest'
import { databasesApi } from './api'
import { connectionColor, errorText, resetDatabases, useDatabases } from './store'
import type { ConnectionsView, RunError, RunOutput, SavedConnection } from './types'

vi.mock('./api', () => ({
  databasesApi: {
    list: vi.fn(),
    run: vi.fn(),
    cancel: vi.fn(),
    setReadOnly: vi.fn(),
  },
}))

const api = vi.mocked(databasesApi)

function saved(id: string, overrides: Partial<SavedConnection> = {}): SavedConnection {
  return {
    id,
    name: `db ${id}`,
    env: null,
    color: null,
    host: 'localhost',
    port: 5432,
    user: 'postgres',
    database: 'app',
    sslMode: 'prefer',
    readOnly: false,
    ...overrides,
  }
}

const view: ConnectionsView = {
  saved: [saved('a', { readOnly: true, color: '#f00' }), saved('b')],
  detected: [{ id: 'c', containerId: 'x', name: 'pg', project: null, port: 5433, running: true }],
  dockerAvailable: true,
}

const output: RunOutput = {
  statements: [{ rows: 1, returnedColumns: true }],
  result: { columns: ['n'], rows: [['1']], truncated: false },
  elapsedMs: 3,
  rolledBack: false,
}

const DRAFTS = 'lsdev.databases.drafts'
const ACTIVE = 'lsdev.databases.active'

beforeEach(() => {
  vi.restoreAllMocks()
  localStorage.clear()
  api.list.mockReset().mockResolvedValue(view)
  api.run.mockReset()
  api.cancel.mockReset().mockResolvedValue(undefined)
  api.setReadOnly.mockReset().mockResolvedValue(undefined)
  resetDatabases()
})

describe('databases store', () => {
  it('select restores the draft of the newly active connection', async () => {
    const db = useDatabases()
    await db.refresh()
    db.select('a')
    db.setDraft('a', 'select 1')
    db.select('b')
    expect(db.draft.value).toBe('')
    db.select('a')
    expect(db.state.activeId).toBe('a')
    expect(db.draft.value).toBe('select 1')
  })

  it('active exposes readOnly and color for saved and detected connections', async () => {
    const db = useDatabases()
    await db.refresh()
    db.select('a')
    expect(db.active.value).toMatchObject({ id: 'a', readOnly: true, color: '#f00' })
    db.select('c')
    expect(db.active.value).toMatchObject({ id: 'c', readOnly: false, color: null })
  })

  it('run sets running during the call and stores the output', async () => {
    let resolve!: (o: RunOutput) => void
    api.run.mockReturnValue(new Promise((r) => (resolve = r)))
    const db = useDatabases()
    db.select('a')
    db.setDraft('a', 'select 1')
    const pending = db.run()
    expect(db.running.value).toBe(true)
    expect(api.run).toHaveBeenCalledWith('a', 'select 1')
    resolve(output)
    await pending
    expect(db.running.value).toBe(false)
    expect(db.output.value).toEqual(output)
    expect(db.error.value).toBeNull()
  })

  it('stores a rejected db error and clears the previous output', async () => {
    const error: RunError = { kind: 'db', message: 'boom', code: '42P01', hint: null, position: 3 }
    api.run.mockResolvedValueOnce(output).mockRejectedValueOnce(error)
    const db = useDatabases()
    db.select('a')
    db.setDraft('a', 'select 1')
    await db.run()
    await db.run()
    expect(db.error.value).toEqual(error)
    expect(db.output.value).toBeNull()
    expect(db.running.value).toBe(false)
  })

  it('wraps a rejection that is not a RunError', async () => {
    api.run.mockRejectedValue('oops')
    const db = useDatabases()
    db.select('a')
    await db.run()
    expect(db.error.value).toEqual({ kind: 'other', message: 'oops' })
  })

  it('does nothing without an active connection or while running', async () => {
    const db = useDatabases()
    await db.run()
    expect(api.run).not.toHaveBeenCalled()
  })

  it('cancel asks the backend to cancel the active connection', async () => {
    const db = useDatabases()
    db.select('a')
    await db.cancel()
    expect(api.cancel).toHaveBeenCalledWith('a')
  })

  it('drafts and active connection survive a store reset through localStorage', () => {
    const db = useDatabases()
    db.select('a')
    db.setDraft('a', 'select 2')
    resetDatabases()
    expect(db.state.activeId).toBe('a')
    expect(db.state.drafts).toEqual({ a: 'select 2' })
    expect(JSON.parse(localStorage.getItem(DRAFTS)!)).toEqual({ a: 'select 2' })
    expect(localStorage.getItem(ACTIVE)).toBe('a')
  })

  it('ignores corrupt persisted drafts', () => {
    localStorage.setItem(DRAFTS, '{not json')
    resetDatabases()
    expect(useDatabases().state.drafts).toEqual({})
  })

  it('does not break select or setDraft when localStorage throws', () => {
    vi.spyOn(Storage.prototype, 'getItem').mockImplementation(() => {
      throw new Error('denied')
    })
    vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => {
      throw new Error('denied')
    })
    resetDatabases()
    const db = useDatabases()
    expect(() => db.select('a')).not.toThrow()
    expect(() => db.setDraft('a', 'x')).not.toThrow()
    expect(db.state.activeId).toBe('a')
    expect(db.draft.value).toBe('x')
  })

  it('refresh drops an active id that no longer exists', async () => {
    const db = useDatabases()
    db.select('gone')
    await db.refresh()
    expect(db.state.activeId).toBeNull()
  })

  it('keeps run state per connection when the user switches during a run', async () => {
    let resolveA!: (o: RunOutput) => void
    api.run.mockImplementation((id: string) =>
      id === 'a' ? new Promise((r) => (resolveA = r)) : Promise.resolve({ ...output, elapsedMs: 9 }),
    )
    const db = useDatabases()
    db.select('a')
    const pendingA = db.run()
    db.select('b')
    expect(db.running.value).toBe(false)
    await db.run()
    expect(db.output.value?.elapsedMs).toBe(9)
    resolveA(output)
    await pendingA
    expect(db.output.value?.elapsedMs).toBe(9)
    db.select('a')
    expect(db.output.value).toEqual(output)
    expect(db.running.value).toBe(false)
  })

  it('a run result finishing after a switch does not show on the other connection', async () => {
    let resolveA!: (o: RunOutput) => void
    api.run.mockReturnValue(new Promise((r) => (resolveA = r)))
    const db = useDatabases()
    db.select('a')
    const pending = db.run()
    db.select('b')
    resolveA(output)
    await pending
    expect(db.output.value).toBeNull()
    db.select('a')
    expect(db.output.value).toEqual(output)
  })

  it('cancel stores a backend failure as an error of that connection', async () => {
    api.cancel.mockRejectedValue(new Error('nope'))
    const db = useDatabases()
    db.select('a')
    await db.cancel()
    expect(db.error.value).toEqual({ kind: 'other', message: 'nope' })
  })

  it('setReadOnly on a detected connection updates the in-memory override', async () => {
    const db = useDatabases()
    await db.refresh()
    db.select('c')
    await db.setReadOnly('c', true)
    expect(api.setReadOnly).toHaveBeenCalledWith('c', true)
    expect(db.active.value?.readOnly).toBe(true)
    resetDatabases()
    expect(db.state.detectedReadOnly).toEqual({})
  })

  it('setReadOnly on a saved connection refreshes the view', async () => {
    const db = useDatabases()
    await db.refresh()
    api.list.mockClear()
    await db.setReadOnly('b', true)
    expect(api.list).toHaveBeenCalledTimes(1)
  })

  it('setReadOnly rethrows a failure and changes nothing', async () => {
    api.setReadOnly.mockRejectedValue('denied')
    const db = useDatabases()
    await db.refresh()
    api.list.mockClear()
    await expect(db.setReadOnly('c', true)).rejects.toBe('denied')
    expect(db.state.detectedReadOnly).toEqual({})
    expect(api.list).not.toHaveBeenCalled()
  })
})

describe('errorText', () => {
  it('connect: cause, with the tunnel hint on a local port other than 5432', () => {
    expect(errorText({ kind: 'connect', host: '127.0.0.1', port: 5433, cause: 'Connection refused.' })).toBe(
      'Could not connect to 127.0.0.1:5433. Connection refused. Is the tunnel running?',
    )
    expect(errorText({ kind: 'connect', host: 'localhost', port: 6543, cause: 'x.' })).toMatch(/Is the tunnel running\?$/)
    expect(errorText({ kind: 'connect', host: '::1', port: 6543, cause: 'x.' })).toMatch(/Is the tunnel running\?$/)
    expect(errorText({ kind: 'connect', host: '127.0.0.1', port: 5432, cause: 'Connection refused.' })).toBe(
      'Could not connect to 127.0.0.1:5432. Connection refused.',
    )
    expect(errorText({ kind: 'connect', host: 'db.example.com', port: 5433, cause: 'x.' })).not.toContain('tunnel')
  })

  it('db: message, SQLSTATE and hint', () => {
    expect(errorText({ kind: 'db', message: 'syntax error', code: '42601', hint: 'Check commas.', position: 3 })).toBe(
      'syntax error\nSQLSTATE 42601\nCheck commas.',
    )
    expect(errorText({ kind: 'db', message: 'boom', code: '22012', hint: null, position: null })).toBe('boom\nSQLSTATE 22012')
  })

  it('db 25006 explains read-only', () => {
    expect(errorText({ kind: 'db', message: 'cannot execute', code: '25006', hint: null, position: null })).toBe(
      'Read-only is on for this connection. Turn it off to write.',
    )
  })

  it('closed, cancelled, other', () => {
    expect(errorText({ kind: 'closed' })).toBe('The connection was closed. Run the query again.')
    expect(errorText({ kind: 'cancelled' })).toBe('Query cancelled.')
    expect(errorText({ kind: 'other', message: 'odd' })).toBe('odd')
  })
})

describe('connectionColor', () => {
  it('uses the explicit color, else the env default', () => {
    expect(connectionColor({ color: '#f00', env: 'production' })).toBe('#f00')
    expect(connectionColor({ color: null, env: 'production' })).toBe('var(--color-danger)')
    expect(connectionColor({ color: null, env: 'staging' })).toBe('var(--color-warm)')
    expect(connectionColor({ color: null, env: 'local' })).toBe('var(--color-accent)')
    expect(connectionColor({ color: null, env: null })).toBe('var(--color-accent)')
  })
})
