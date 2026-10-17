import { mount, flushPromises } from '@vue/test-utils'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { databasesApi } from './api'
import ConnectionDialog from './ConnectionDialog.vue'
import { resetDatabases } from './store'
import type { ConnectionDraft, ParsedUrl } from './types'

vi.mock('./api', () => ({ databasesApi: { list: vi.fn(), parseUrl: vi.fn(), save: vi.fn(), test: vi.fn() } }))
const api = vi.mocked(databasesApi)

const parsed: ParsedUrl = { name: 'shop', env: null, color: null, host: 'db.example', port: 6543, user: 'bob', password: 'pw', database: 'shopdb', sslMode: 'require' }
const existing: ConnectionDraft = { id: 'x1', name: 'Mine', env: 'local', color: '#112233', host: 'h', port: 5432, user: 'u', database: 'd', sslMode: 'prefer', readOnly: false }

beforeEach(() => {
  vi.resetAllMocks()
  localStorage.clear()
  resetDatabases()
  api.list.mockResolvedValue({ saved: [], detected: [], dockerAvailable: true })
})

function open(initial: ConnectionDraft | null = null) {
  return mount(ConnectionDialog, { props: { open: true, initial }, attachTo: document.body })
}
const val = (w: ReturnType<typeof open>, id: string) => (w.find(`#${id}`).element as HTMLInputElement).value

describe('ConnectionDialog', () => {
  it('renders nothing when closed', () => {
    const w = mount(ConnectionDialog, { props: { open: false, initial: null } })
    expect(w.find('[role="dialog"]').exists()).toBe(false)
  })

  it('pasting a URL fills the fields from parseUrl', async () => {
    api.parseUrl.mockResolvedValue(parsed)
    const w = open()
    const input = w.find('#conn-url')
    await input.trigger('paste', { clipboardData: { getData: () => 'postgres://bob:pw@db.example:6543/shopdb' } })
    await flushPromises()
    expect(api.parseUrl).toHaveBeenCalledWith('postgres://bob:pw@db.example:6543/shopdb')
    expect(val(w, 'conn-host')).toBe('db.example')
    expect(val(w, 'conn-port')).toBe('6543')
    expect(val(w, 'conn-user')).toBe('bob')
    expect(val(w, 'conn-password')).toBe('pw')
    expect(val(w, 'conn-database')).toBe('shopdb')
    expect(val(w, 'conn-name')).toBe('shop')
    expect(val(w, 'conn-ssl')).toBe('require')
  })

  it('clears the URL field after parsing and keeps the password out of the HTML', async () => {
    api.parseUrl.mockResolvedValue(parsed)
    const w = open()
    await w.find('#conn-url').trigger('paste', { clipboardData: { getData: () => 'postgres://bob:pw@db.example:6543/shopdb' } })
    await flushPromises()
    expect(val(w, 'conn-url')).toBe('')
    expect(val(w, 'conn-password')).toBe('pw')
    expect(w.html()).not.toContain('postgres://bob:pw')
  })

  it('does not overwrite a typed password when the URL has none', async () => {
    api.parseUrl.mockResolvedValue({ ...parsed, password: null })
    const w = open()
    await w.find('#conn-password').setValue('typed')
    await w.find('#conn-url').setValue('postgres://bob@db.example/shopdb')
    await w.find('#conn-url').trigger('blur')
    await flushPromises()
    expect(val(w, 'conn-password')).toBe('typed')
  })

  it('ignores an out-of-order parse result', async () => {
    let first!: (p: ParsedUrl) => void
    api.parseUrl.mockImplementationOnce(() => new Promise((r) => (first = r)))
    api.parseUrl.mockResolvedValueOnce({ ...parsed, host: 'second' })
    const w = open()
    await w.find('#conn-url').setValue('a')
    await w.find('#conn-url').trigger('blur')
    await w.find('#conn-url').setValue('b')
    await w.find('#conn-url').trigger('blur')
    await flushPromises()
    first({ ...parsed, host: 'first' })
    await flushPromises()
    expect(val(w, 'conn-host')).toBe('second')
  })

  it('opening an existing staging connection keeps read-only off', async () => {
    api.save.mockResolvedValue({ ...existing, id: 'x1' })
    const w = mount(ConnectionDialog, { props: { open: false, initial: null }, attachTo: document.body })
    await w.setProps({ open: true, initial: { ...existing, env: 'staging', readOnly: false } })
    await flushPromises()
    expect(w.find('[role="switch"]').attributes('aria-checked')).toBe('false')
    await w.find('form').trigger('submit')
    await flushPromises()
    expect(api.save.mock.calls[0][0].readOnly).toBe(false)
  })

  it('focuses the URL field for a new connection and the name for an edit', async () => {
    const a = open()
    await flushPromises()
    expect(document.activeElement).toBe(a.find('#conn-url').element)
    const b = open(existing)
    await flushPromises()
    expect(document.activeElement).toBe(b.find('#conn-name').element)
  })

  it('shows the parseUrl rejection under the input', async () => {
    api.parseUrl.mockRejectedValue('Unsupported sslmode')
    const w = open()
    await w.find('#conn-url').setValue('postgres://x')
    await w.find('#conn-url').trigger('blur')
    await flushPromises()
    expect(w.find('[data-testid="url-error"]').text()).toBe('Unsupported sslmode')
  })

  it('does not re-parse an unchanged value on blur', async () => {
    api.parseUrl.mockResolvedValue(parsed)
    const w = open()
    await w.find('#conn-url').setValue('postgres://x')
    await w.find('#conn-url').trigger('blur')
    await w.find('#conn-url').trigger('blur')
    await flushPromises()
    expect(api.parseUrl).toHaveBeenCalledTimes(1)
  })

  it('env staging turns read-only on', async () => {
    const w = open()
    await w.find('#conn-env').setValue('staging')
    expect(w.find('[role="switch"]').attributes('aria-checked')).toBe('true')
  })

  it('toggling the switch first prevents the env default', async () => {
    const w = open()
    const sw = w.find('[role="switch"]')
    await sw.trigger('click')
    await sw.trigger('click')
    expect(sw.attributes('aria-checked')).toBe('false')
    await w.find('#conn-env').setValue('production')
    expect(sw.attributes('aria-checked')).toBe('false')
  })

  it('edit mode shows Unchanged and sends password null when empty', async () => {
    api.save.mockResolvedValue({ ...existing, id: 'x1' })
    const w = open(existing)
    expect(w.find('#conn-password').attributes('placeholder')).toBe('Unchanged')
    expect(val(w, 'conn-name')).toBe('Mine')
    await w.find('form').trigger('submit')
    await flushPromises()
    expect(api.save).toHaveBeenCalledTimes(1)
    expect(api.save.mock.calls[0][0]).toMatchObject({ id: 'x1', name: 'Mine', host: 'h', port: 5432, color: '#112233' })
    expect(api.save.mock.calls[0][1]).toBeNull()
    expect(w.emitted('saved')).toBeTruthy()
    expect(api.list).toHaveBeenCalled()
  })

  it('sends the typed password on save', async () => {
    api.save.mockResolvedValue({ ...existing, id: 'x1' })
    const w = open(existing)
    await w.find('#conn-password').setValue('secret')
    await w.find('form').trigger('submit')
    await flushPromises()
    expect(api.save.mock.calls[0][1]).toBe('secret')
  })

  it('Default color checkbox sends color null', async () => {
    api.save.mockResolvedValue({ ...existing, id: 'x1' })
    const w = open(existing)
    await w.find('#conn-color-default').setValue(true)
    await w.find('form').trigger('submit')
    await flushPromises()
    expect(api.save.mock.calls[0][0].color).toBeNull()
  })

  it('save failure is shown and the dialog stays open', async () => {
    api.save.mockRejectedValue('Name is required')
    const w = open()
    await w.find('form').trigger('submit')
    await flushPromises()
    expect(w.find('[data-testid="form-error"]').text()).toBe('Name is required')
    expect(w.emitted('saved')).toBeFalsy()
  })

  it('Test connection shows Connected', async () => {
    api.test.mockResolvedValue()
    const w = open(existing)
    await w.find('button[data-action="test"]').trigger('click')
    await flushPromises()
    expect(api.test.mock.calls[0][1]).toBeNull()
    expect(w.find('[data-testid="test-result"]').text()).toBe('Connected')
  })

  it('Test connection shows the backend error as is', async () => {
    api.test.mockRejectedValue('Could not connect to h:5432.')
    const w = open(existing)
    await w.find('button[data-action="test"]').trigger('click')
    await flushPromises()
    expect(w.find('[data-testid="test-result"]').text()).toBe('Could not connect to h:5432.')
  })

  it.each([['0'], ['65536'], ['12.5'], ['']])('refuses port %s before saving or testing', async (port) => {
    const w = open(existing)
    await w.find('#conn-port').setValue(port)
    await w.find('form').trigger('submit')
    await flushPromises()
    expect(api.save).not.toHaveBeenCalled()
    expect(w.find('[data-testid="form-error"]').text()).toBe('Port must be between 1 and 65535.')
    await w.find('button[data-action="test"]').trigger('click')
    await flushPromises()
    expect(api.test).not.toHaveBeenCalled()
    expect(w.find('[data-testid="test-result"]').text()).toBe('Port must be between 1 and 65535.')
  })

  it('accepts port 65535', async () => {
    api.save.mockResolvedValue({ ...existing, id: 'x1' })
    const w = open(existing)
    await w.find('#conn-port').setValue('65535')
    await w.find('form').trigger('submit')
    await flushPromises()
    expect(api.save.mock.calls[0][0].port).toBe(65535)
  })

  it.each([
    ['conn-host', 'other-host'],
    ['conn-port', '6543'],
    ['conn-user', 'admin'],
  ])('edit: changing %s without a password asks to enter it again', async (field, value) => {
    const w = open(existing)
    expect(w.find('[data-testid="password-hint"]').exists()).toBe(false)
    await w.find(`#${field}`).setValue(value)
    expect(w.find('[data-testid="password-hint"]').text()).toBe('Enter the password again for the new server.')
    await w.find('#conn-password').setValue('pw')
    expect(w.find('[data-testid="password-hint"]').exists()).toBe(false)
  })

  it('edit: other changes and a new connection show no password hint', async () => {
    const w = open(existing)
    await w.find('#conn-name').setValue('Renamed')
    await w.find('#conn-database').setValue('other_db')
    await w.find('#conn-host').setValue(' h ')
    expect(w.find('[data-testid="password-hint"]').exists()).toBe(false)
    const n = open()
    await n.find('#conn-host').setValue('other-host')
    expect(n.find('[data-testid="password-hint"]').exists()).toBe(false)
  })

  it('Cancel emits close', async () => {
    const w = open()
    await w.find('button[data-action="cancel"]').trigger('click')
    expect(w.emitted('close')).toBeTruthy()
  })
})
