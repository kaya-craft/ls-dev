import { reactive } from 'vue'
import { coreApi } from '../../core/api'
import type { Settings } from '../../core/types'

const state = reactive({ settings: null as Settings | null })

export async function loadSettings() {
  state.settings = await coreApi.getSettings()
}

/** Saves a patch; the backend returns the normalized settings. Throws before the first load. */
export async function updateSettings(patch: Partial<Settings>) {
  if (!state.settings) throw new Error('Settings are not loaded yet.')
  state.settings = await coreApi.setSettings({ ...state.settings, ...patch })
}

export function useSettings() {
  return state
}
