import { beforeEach, describe, expect, it, vi } from 'vitest'
import { killService, restartService, runComposeAction, runContainerAction } from './actions'
import { api } from './api'
import { confirm } from '../../ui/store/confirm'
import { toasts } from '../../ui/store/toasts'
import { ctr, proc } from '../../test/fixtures'

vi.mock('./api', () => ({
  api: {
    killProcess: vi.fn(),
    restartProcess: vi.fn(),
    containerAction: vi.fn(),
    composeAction: vi.fn(),
    openBrowser: vi.fn(),
    openEditor: vi.fn(),
    openTerminal: vi.fn(),
  },
}))
vi.mock('../../ui/store/confirm', () => ({ confirm: vi.fn() }))
vi.mock('../../ui/store/toasts', () => ({ toasts: { success: vi.fn(), error: vi.fn() } }))

const confirmMock = vi.mocked(confirm)

describe('actions', () => {
  beforeEach(() => vi.clearAllMocks())

  it('does nothing when the kill is cancelled', async () => {
    confirmMock.mockResolvedValue({ ok: false, checked: false })
    await killService(proc())
    expect(api.killProcess).not.toHaveBeenCalled()
  })

  it('lists every process of the tree in the kill confirmation', async () => {
    confirmMock.mockResolvedValue({ ok: false, checked: false })
    await killService(proc())
    expect(confirmMock.mock.calls[0][0].details).toHaveLength(2)
  })

  it('reports survivors as an error', async () => {
    confirmMock.mockResolvedValue({ ok: true, checked: false })
    vi.mocked(api.killProcess).mockResolvedValue({ survivors: [42] })
    await killService(proc())
    expect(toasts.error).toHaveBeenCalledWith(expect.stringContaining('42'))
  })

  it('shows the command and the environment caveat before a restart', async () => {
    confirmMock.mockResolvedValue({ ok: true, checked: false })
    vi.mocked(api.restartProcess).mockResolvedValue()
    await restartService(proc())
    const req = confirmMock.mock.calls[0][0]
    expect(req.body).toContain('pnpm --filter @acme/backend dev')
    expect(req.body).toContain('not reproduced')
    expect(api.restartProcess).toHaveBeenCalledWith(proc().id)
  })

  it('passes the Force checkbox to remove', async () => {
    confirmMock.mockResolvedValue({ ok: true, checked: true })
    vi.mocked(api.containerAction).mockResolvedValue()
    await runContainerAction(ctr(), 'remove')
    expect(api.containerAction).toHaveBeenCalledWith('docker:aaa', 'remove', true)
  })

  it('starts a container without confirmation and surfaces Docker errors', async () => {
    vi.mocked(api.containerAction).mockRejectedValue('port is already allocated')
    await runContainerAction(ctr(), 'start')
    expect(confirmMock).not.toHaveBeenCalled()
    expect(toasts.error).toHaveBeenCalledWith('port is already allocated')
  })

  it('confirms before stopping a whole Compose project', async () => {
    confirmMock.mockResolvedValue({ ok: false, checked: false })
    await runComposeAction({ project: 'p', workingDir: null, containers: [ctr()] }, 'stop')
    expect(api.composeAction).not.toHaveBeenCalled()
  })

  it('surfaces a failing editor command', async () => {
    vi.mocked(api.openEditor).mockRejectedValue('The editor command failed (zsh: command not found: cursor). Check Settings.')
    const { openEditor } = await import('./actions')
    await openEditor(proc())
    expect(toasts.error).toHaveBeenCalledWith(expect.stringContaining('Check Settings'))
  })
})
