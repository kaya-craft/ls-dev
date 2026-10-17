// The only module of the Worktrees tool that talks to Tauri. Tests mock it with vi.mock('./api').
import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import type { Worktree } from '../services/types'
import type { RemoveMergedReport, WorktreesSnapshot } from './types'

export const worktreesApi = {
  get: () => invoke<WorktreesSnapshot>('worktrees_get'),
  scan: () => invoke<void>('worktrees_scan'),
  removeMerged: (paths: string[]) => invoke<RemoveMergedReport>('worktrees_remove_merged', { paths }),
  removeWorktree: (worktree: Worktree, force = false) => invoke<void>('remove_worktree', { worktree, force }),
  openTerminal: (worktree: Worktree) => invoke<void>('open_worktree_terminal', { worktree }),
  openShell: (path: string) => invoke<void>('open_worktree_shell', { path }),
  onChanged: (cb: (snapshot: WorktreesSnapshot) => void): Promise<UnlistenFn> =>
    listen<WorktreesSnapshot>('worktrees:changed', (event) => cb(event.payload)),
  onError: (cb: (message: string) => void): Promise<UnlistenFn> =>
    listen<{ message: string }>('worktrees:error', (event) => cb(event.payload.message)),
}
