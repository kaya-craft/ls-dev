import { api, type ContainerActionName } from './api'
import { confirm } from '../../ui/store/confirm'
import { toasts } from '../../ui/store/toasts'
import type { ComposeGroup, ContainerService, ProcessService, Service } from './types'

function message(error: unknown): string {
  return typeof error === 'string' ? error : error instanceof Error ? error.message : String(error)
}

export async function killService(p: ProcessService) {
  const { ok } = await confirm({
    title: `Kill ${p.project}?`,
    body: `${p.tree.length} process(es) will receive SIGTERM, then SIGKILL after 3 seconds.`,
    details: p.tree.map((n) => `${n.depth === 0 ? '→' : ' '} ${n.pid}  ${n.command}`),
    confirmLabel: 'Kill',
    danger: true,
  })
  if (!ok) return
  try {
    const { survivors } = await api.killProcess(p.id)
    if (survivors.length > 0) toasts.error(`Some processes survived SIGKILL: ${survivors.join(', ')}`)
    else toasts.success(`${p.project} stopped`)
  } catch (error) {
    toasts.error(message(error))
  }
}

export async function restartService(p: ProcessService) {
  if (!p.restart) return
  const { ok } = await confirm({
    title: `Restart ${p.project}?`,
    body:
      `LsDev will stop the process tree and run "${p.restart.argv.join(' ')}" in ${p.restart.cwd}. ` +
      'Environment variables exported in the original terminal are not reproduced. ' +
      'Logs will then be available in LsDev.',
    confirmLabel: 'Restart',
  })
  if (!ok) return
  try {
    await api.restartProcess(p.id)
    toasts.success(`${p.project} restarted`)
  } catch (error) {
    toasts.error(message(error))
  }
}

export async function runContainerAction(c: ContainerService, action: ContainerActionName) {
  let force = false
  if (action === 'remove') {
    const { ok, checked } = await confirm({
      title: `Remove ${c.name}?`,
      body: 'The container is deleted. Its image and named volumes are kept.',
      confirmLabel: 'Remove',
      danger: true,
      checkbox: 'Force (stop it first if running)',
    })
    if (!ok) return
    force = checked
  }
  try {
    await api.containerAction(c.id, action, force)
  } catch (error) {
    toasts.error(message(error))
  }
}

export async function runComposeAction(group: ComposeGroup, action: 'start' | 'stop') {
  if (action === 'stop') {
    const { ok } = await confirm({
      title: `Stop ${group.project}?`,
      body: `${group.containers.length} container(s) will be stopped.`,
      details: group.containers.map((c) => c.name),
      confirmLabel: 'Stop all',
      danger: true,
    })
    if (!ok) return
  }
  try {
    await api.composeAction(group.project, action)
  } catch (error) {
    toasts.error(message(error))
  }
}

async function attempt(run: () => Promise<void>) {
  try {
    await run()
  } catch (error) {
    toasts.error(message(error))
  }
}

export const openBrowser = (port: number) => attempt(() => api.openBrowser(port))
export const openEditor = (service: Service) => attempt(() => api.openEditor(service.id))
export const openTerminal = (c: ContainerService) => attempt(() => api.openTerminal(c.id))
