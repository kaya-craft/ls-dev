<script setup lang="ts">
import { onMounted, onUnmounted, ref } from 'vue'
import { api } from './tools/services/api'
import { applyInitialSnapshot, applySnapshot, select, setError } from './tools/services/store/services'
import { worktreesApi } from './tools/worktrees/api'
import { applyInitialWorktrees, applyWorktrees, setWorktreesError } from './tools/worktrees/store'
import { tunnelsApi } from './tools/tunnels/api'
import { applyTunnels, closeShare, refresh as refreshTunnels, useTunnels } from './tools/tunnels/store'
import { loadSettings } from './ui/store/settings'
import { handleChunk, handleClosed } from './tools/services/store/logs'
import { answer, useConfirm } from './ui/store/confirm'
import { handleShortcut } from './shell/shortcuts'
import { isNarrow } from './shell/viewport'
import { setTool, toggleSidebar, useNavigation } from './shell/navigation'
import AppSidebar from './shell/AppSidebar.vue'
import ServicesPage from './tools/services/ServicesPage.vue'
import WorktreesPage from './tools/worktrees/WorktreesPage.vue'
import DatabasesPage from './tools/databases/DatabasesPage.vue'
import TunnelsPage from './tools/tunnels/TunnelsPage.vue'
import ConfirmDialog from './ui/ConfirmDialog.vue'
import Toasts from './ui/Toasts.vue'
import SettingsDialog from './ui/SettingsDialog.vue'
import ShareDialog from './tools/tunnels/ShareDialog.vue'

const unlisten: Array<() => void> = []
const confirmState = useConfirm()
const settingsOpen = ref(false)
const { state: tunnelsState } = useTunnels()

const { state: nav } = useNavigation()
const servicesPage = ref<InstanceType<typeof ServicesPage> | null>(null)

// Esc cancels an open confirmation, then closes the settings dialog, then the detail pane.
function onKeydown(event: KeyboardEvent) {
  handleShortcut(event, {
    refresh: () => void api.refresh(),
    focusFilter: () => servicesPage.value?.focusFilter(),
    openSettings: () => {
      settingsOpen.value = true
    },
    escape: () => {
      if (confirmState.request) answer({ ok: false, checked: false })
      else if (tunnelsState.share.open) closeShare()
      else if (settingsOpen.value) settingsOpen.value = false
      else select(null)
    },
    switchTool: setTool,
    toggleSidebar,
  }, { tool: nav.currentTool, modalOpen: Boolean(confirmState.request) || settingsOpen.value || tunnelsState.share.open, narrow: isNarrow.value })
}

onMounted(async () => {
  window.addEventListener('keydown', onKeydown)
  try {
    unlisten.push(await api.onSnapshot(applySnapshot))
    unlisten.push(await api.onCollectError(setError))
    unlisten.push(await api.onLogChunk(handleChunk))
    unlisten.push(await api.onLogClosed(handleClosed))
  } catch (e) {
    setError(String(e))
  }
  try {
    unlisten.push(await worktreesApi.onChanged(applyWorktrees))
  } catch (e) {
    setWorktreesError(String(e))
  }
  try {
    unlisten.push(await tunnelsApi.onChanged(applyTunnels))
  } catch (e) {
    setError(String(e))
  }
  try {
    unlisten.push(await worktreesApi.onError(setWorktreesError))
  } catch (e) {
    setWorktreesError(String(e))
  }
  const results = await Promise.allSettled([
    api.getSnapshot().then(applyInitialSnapshot),
    loadSettings(),
    worktreesApi.get().then(applyInitialWorktrees).catch((e) => setWorktreesError(String(e))),
    refreshTunnels().catch(() => {}),
  ])
  for (const r of results) if (r.status === 'rejected') setError(String(r.reason))
})
onUnmounted(() => {
  window.removeEventListener('keydown', onKeydown)
  unlisten.forEach((fn) => fn())
})
</script>

<template>
  <div class="flex h-screen bg-bg text-ink">
    <AppSidebar />
    <div class="flex min-w-0 flex-1 flex-col">
      <ServicesPage v-if="nav.currentTool === 'services'" ref="servicesPage" @open-settings="settingsOpen = true" />
      <WorktreesPage v-else-if="nav.currentTool === 'worktrees'" @open-settings="settingsOpen = true" />
      <DatabasesPage v-else-if="nav.currentTool === 'databases'" @open-settings="settingsOpen = true" />
      <TunnelsPage v-else @open-settings="settingsOpen = true" />
    </div>
    <ConfirmDialog />
    <SettingsDialog :open="settingsOpen" @close="settingsOpen = false" />
    <ShareDialog />
    <Toasts />
  </div>
</template>
