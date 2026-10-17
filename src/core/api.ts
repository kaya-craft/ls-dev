// Tauri calls shared by every tool. Tests mock it with vi.mock('…/core/api').
import { invoke } from '@tauri-apps/api/core'
import type { Settings } from './types'

export const coreApi = {
  getSettings: () => invoke<Settings>('get_settings'),
  setSettings: (settings: Settings) => invoke<Settings>('set_settings', { settings }),
}
