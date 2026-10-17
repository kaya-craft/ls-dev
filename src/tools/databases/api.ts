// The only module of the Databases tool that talks to Tauri. Tests mock it with vi.mock('./api').
import { invoke } from '@tauri-apps/api/core'
import type { ConnectionDraft, ConnectionsView, ParsedUrl, RunOutput, SavedConnection } from './types'

export const databasesApi = {
  list: () => invoke<ConnectionsView>('databases_list'),
  parseUrl: (url: string) => invoke<ParsedUrl>('databases_parse_url', { url }),
  save: (draft: ConnectionDraft, password?: string | null) =>
    invoke<SavedConnection>('databases_save', { draft, password: password ?? null }),
  remove: (id: string) => invoke<void>('databases_delete', { id }),
  test: (draft: ConnectionDraft, password?: string | null) =>
    invoke<void>('databases_test', { draft, password: password ?? null }),
  setReadOnly: (id: string, on: boolean) => invoke<void>('databases_set_read_only', { id, on }),
  run: (id: string, sql: string) => invoke<RunOutput>('databases_run', { id, sql }),
  cancel: (id: string) => invoke<void>('databases_cancel', { id }),
  history: (id: string) => invoke<string[]>('databases_history', { id }),
  clearHistory: (id: string) => invoke<void>('databases_clear_history', { id }),
}
