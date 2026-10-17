// The only module that talks to Tauri. Tests mock it with vi.mock('../api').
import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import type { Snapshot } from './types'

export type LogLine = { stream: 'stdout' | 'stderr'; line: string }
export type LogChunk = { id: string; lines: LogLine[] }
export type ContainerActionName = 'start' | 'stop' | 'restart' | 'remove'

export const api = {
  getSnapshot: () => invoke<Snapshot>('get_snapshot'),
  refresh: () => invoke<void>('refresh'),
  onSnapshot: (cb: (snapshot: Snapshot) => void): Promise<UnlistenFn> =>
    listen<Snapshot>('services:changed', (event) => cb(event.payload)),
  onCollectError: (cb: (message: string) => void): Promise<UnlistenFn> =>
    listen<{ message: string }>('collect:error', (event) => cb(event.payload.message)),
  killProcess: (id: string) => invoke<{ survivors: number[] }>('kill_process', { id }),
  restartProcess: (id: string) => invoke<void>('restart_process', { id }),
  containerAction: (id: string, action: ContainerActionName, force = false) =>
    invoke<void>('container_action', { id, action, force }),
  composeAction: (project: string, action: 'start' | 'stop') =>
    invoke<void>('compose_action', { project, action }),
  openBrowser: (port: number) => invoke<void>('open_browser', { port }),
  openEditor: (id: string) => invoke<void>('open_editor', { id }),
  openTerminal: (id: string) => invoke<void>('open_terminal', { id }),
  logsOpen: (id: string) => invoke<void>('logs_open', { id }),
  logsClose: () => invoke<void>('logs_close'),
  onLogChunk: (cb: (chunk: LogChunk) => void): Promise<UnlistenFn> =>
    listen<LogChunk>('logs:chunk', (event) => cb(event.payload)),
  onLogClosed: (cb: (e: { id: string; reason: string }) => void): Promise<UnlistenFn> =>
    listen<{ id: string; reason: string }>('logs:closed', (event) => cb(event.payload)),
}
