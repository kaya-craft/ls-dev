<script setup lang="ts">
import { ref } from 'vue'
import { killService, openEditor, openTerminal, runComposeAction, runContainerAction } from './actions'
import { removeWorktree } from '../worktrees/actions'
import { servicesInPath } from '../worktrees/store'
import { useServices } from './store/services'
import type { ProcessService } from './types'
import { BTN, BTN_DANGER, BTN_GHOST, BTN_ROW, BTN_ROW_DANGER } from '../../ui/layout'
import ServicesHeader from './ServicesHeader.vue'
import StatTiles from './StatTiles.vue'
import ServiceTable from './ServiceTable.vue'
import DetailPane from './DetailPane.vue'
import PortMenu from './PortMenu.vue'

defineEmits<{ 'open-settings': [] }>()
const header = ref<InstanceType<typeof ServicesHeader> | null>(null)
defineExpose({ focusFilter: () => header.value?.focusFilter() })

function onRemoveWorktree(p: ProcessService) {
  if (!p.worktree) return
  const processes = useServices().state.snapshot?.processes ?? []
  removeWorktree(p.worktree, servicesInPath(p.worktree.path, processes))
}
</script>

<template>
  <div class="flex min-h-0 flex-1 flex-col">
    <ServicesHeader ref="header" @open-settings="$emit('open-settings')" />
    <StatTiles />
    <main class="relative flex min-h-0 flex-1">
      <div class="mx-4 mb-4 min-w-0 flex-1 overflow-auto rounded-tile border border-line bg-card shadow-card">
        <ServiceTable>
          <template #process-actions="{ service }">
            <PortMenu compact :ports="service.ports" />
            <button :class="BTN_ROW_DANGER" @click="killService(service)">Kill</button>
          </template>
          <template #container-actions="{ service }">
            <PortMenu compact v-if="service.state === 'running'" :ports="service.ports" />
            <button v-if="service.state === 'running'" :class="BTN_ROW" @click="runContainerAction(service, 'stop')">Stop</button>
            <button v-else :class="BTN_ROW" @click="runContainerAction(service, 'start')">Start</button>
          </template>
          <template #group-actions="{ group }">
            <button :class="BTN_GHOST" @click="runComposeAction(group, 'start')">Start all</button>
            <button :class="BTN_GHOST" @click="runComposeAction(group, 'stop')">Stop all</button>
          </template>
        </ServiceTable>
      </div>
      <DetailPane>
        <template #process-extra="{ service }">
          <PortMenu :ports="service.ports" />
          <button :class="BTN" :disabled="!service.cwd" @click="openEditor(service)">Open project</button>
          <button v-if="service.worktree" :class="BTN_DANGER" @click="onRemoveWorktree(service)">Remove worktree</button>
        </template>
        <template #container-extra="{ service }">
          <PortMenu v-if="service.state === 'running'" :ports="service.ports" />
          <button :class="BTN" :disabled="service.state !== 'running'" @click="openTerminal(service)">Open terminal</button>
          <button :class="BTN" :disabled="!service.compose?.workingDir" @click="openEditor(service)">Open project</button>
        </template>
      </DetailPane>
    </main>
  </div>
</template>
