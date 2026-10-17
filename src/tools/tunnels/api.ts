// The only module of the Tunnels tool that talks to Tauri. Tests mock it with vi.mock('./api').
import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import type { OpenRequest, SuggestedName, Tunnel, TunnelsStatus } from './types'

export const tunnelsApi = {
  status: () => invoke<TunnelsStatus>('tunnels_status'),
  list: () => invoke<Tunnel[]>('tunnels_list'),
  suggestName: (cwd: string | null, project: string) => invoke<SuggestedName>('tunnels_suggest_name', { cwd, project }),
  open: (request: OpenRequest) => invoke<Tunnel>('tunnels_open', { request }),
  extend: (id: string) => invoke<Tunnel>('tunnels_extend', { id }),
  stop: (id: string) => invoke<void>('tunnels_stop', { id }),
  output: (id: string) => invoke<string[]>('tunnels_output', { id }),
  deleteStable: (cwd: string) => invoke<void>('tunnels_delete_stable', { cwd }),
  login: () => invoke<void>('tunnels_login'),
  onChanged: (cb: (tunnels: Tunnel[]) => void): Promise<UnlistenFn> =>
    listen<Tunnel[]>('tunnels:changed', (event) => cb(event.payload)),
}
