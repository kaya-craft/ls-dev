import { mount } from '@vue/test-utils'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { toasts } from '../../ui/store/toasts'
import ResultGrid from './ResultGrid.vue'
import { toCsv } from './copy'
import type { RunError, RunOutput } from './types'

vi.mock('../../ui/store/toasts', () => ({ toasts: { success: vi.fn(), error: vi.fn() } }))

const writeText = vi.fn().mockResolvedValue(undefined)

beforeEach(() => {
  vi.clearAllMocks()
  writeText.mockResolvedValue(undefined)
  Object.defineProperty(navigator, 'clipboard', { value: { writeText }, configurable: true })
})

function select(columns: string[], rows: (string | null)[][], extra: Partial<RunOutput['result'] & object> = {}, rolledBack = false): RunOutput {
  return {
    statements: [{ rows: rows.length, returnedColumns: true }],
    result: { columns, rows, truncated: false, ...extra },
    elapsedMs: 12,
    rolledBack,
  }
}

function mountGrid(output: RunOutput | null, error: RunError | null = null, running = false) {
  return mount(ResultGrid, { props: { output, error, running }, attachTo: document.body })
}

const cells = (w: ReturnType<typeof mountGrid>) => w.findAll('[data-testid="cell"]')

describe('ResultGrid', () => {
  it('renders NULL in italics and an empty string as empty', () => {
    const w = mountGrid(select(['a', 'b'], [[null, '']]))
    const [nul, empty] = cells(w)
    expect(nul.text()).toBe('NULL')
    expect(nul.classes()).toContain('italic')
    expect(nul.classes()).toContain('text-ink-3')
    expect(empty.text()).toBe('')
  })

  it('truncates values over 200 characters and keeps the full value in title', () => {
    const long = 'x'.repeat(300)
    const w = mountGrid(select(['a'], [[long]]))
    const cell = cells(w)[0]
    expect(cell.text()).toBe('x'.repeat(200) + '…')
    expect(cell.attributes('title')).toBe(long)
  })

  it('shows the truncated banner', () => {
    const w = mountGrid(select(['a'], [['1']], { truncated: true }))
    expect(w.text()).toContain('Showing the first 1,000 rows; the query was stopped. Changes made by this batch, if any, were undone.')
  })

  it('shows the rolled back banner', () => {
    const w = mountGrid(select(['a'], [['1']], {}, true))
    expect(w.text()).toContain('Earlier statements in this batch were rolled back.')
  })

  it('status line for a select', () => {
    const w = mountGrid(select(['a'], [['1'], ['2']]))
    expect(w.get('[data-testid="status"]').text()).toBe('2 rows · 12 ms')
  })

  it('status line for two updates', () => {
    const w = mountGrid({
      statements: [
        { rows: 3, returnedColumns: false },
        { rows: 1, returnedColumns: false },
      ],
      result: null,
      elapsedMs: 8,
      rolledBack: false,
    })
    expect(w.get('[data-testid="status"]').text()).toBe('3 rows affected · 1 row affected · 8 ms')
  })

  it('status line uses the singular for one row', () => {
    const w = mountGrid(select(['a'], [['1']]))
    expect(w.get('[data-testid="status"]').text()).toBe('1 row · 12 ms')
  })

  it('Shift+click selects a rectangle and Cmd+C copies it as TSV', async () => {
    const w = mountGrid(select(['a', 'b', 'c'], [['1', '2', '3'], ['4', null, '6'], ['7', '8', '9']]))
    const all = cells(w)
    await all[0].trigger('click')
    await all[4].trigger('click', { shiftKey: true })
    expect(all[0].classes()).toContain('bg-accent-soft')
    expect(all[4].classes()).toContain('bg-accent-soft')
    expect(all[8].classes()).not.toContain('bg-accent-soft')
    await w.get('[data-testid="grid"]').trigger('keydown', { key: 'c', metaKey: true })
    expect(writeText).toHaveBeenCalledWith('1\t2\n4\t')
    await vi.waitFor(() => expect(toasts.success).toHaveBeenCalledWith('Copied'))
  })

  it('Cmd+C on one cell copies the raw value, null as empty', async () => {
    const w = mountGrid(select(['a', 'b'], [['hello\tworld', null]]))
    const all = cells(w)
    await all[0].trigger('click')
    await w.get('[data-testid="grid"]').trigger('keydown', { key: 'c', metaKey: true })
    expect(writeText).toHaveBeenLastCalledWith('hello\tworld')
    await all[1].trigger('click')
    await w.get('[data-testid="grid"]').trigger('keydown', { key: 'c', metaKey: true })
    expect(writeText).toHaveBeenLastCalledWith('')
  })

  it('clicking a row number selects the whole row', async () => {
    const w = mountGrid(select(['a', 'b'], [['1', '2'], ['3', '4']]))
    await w.findAll('[data-testid="row-number"]')[1].trigger('click')
    await w.get('[data-testid="grid"]').trigger('keydown', { key: 'c', ctrlKey: true })
    expect(writeText).toHaveBeenCalledWith('3\t4')
  })

  it('Copy as CSV writes the whole result', async () => {
    const out = select(['a', 'b'], [['1', 'x,y']])
    const w = mountGrid(out)
    await w.get('[data-testid="copy-as"]').trigger('click')
    await w.get('[data-testid="copy-csv"]').trigger('click')
    expect(writeText).toHaveBeenCalledWith(toCsv(['a', 'b'], [['1', 'x,y']]))
  })

  it('Copy as menu closes on Escape and on outside click', async () => {
    const w = mountGrid(select(['a'], [['1']]))
    await w.get('[data-testid="copy-as"]').trigger('click')
    expect(w.find('[data-testid="copy-csv"]').exists()).toBe(true)
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }))
    await w.vm.$nextTick()
    expect(w.find('[data-testid="copy-csv"]').exists()).toBe(false)
    await w.get('[data-testid="copy-as"]').trigger('click')
    document.body.click()
    await w.vm.$nextTick()
    expect(w.find('[data-testid="copy-csv"]').exists()).toBe(false)
  })

  it('shows the read-only text for 25006', () => {
    const w = mountGrid(null, { kind: 'db', message: 'cannot execute', code: '25006', hint: null, position: null })
    expect(w.text()).toContain('Read-only is on for this connection. Turn it off to write.')
  })

  it('shows the running indicator over the previous result', () => {
    const w = mountGrid(select(['a'], [['1']]), null, true)
    expect(w.text()).toContain('Running…')
    expect(cells(w)).toHaveLength(1)
  })
})
