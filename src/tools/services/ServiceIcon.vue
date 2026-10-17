<script setup lang="ts">
import { computed } from 'vue'
import type { ContainerService, ProcessService } from './types'
import { iconFor, type IconTone } from './rowText'

const props = defineProps<{ service: ProcessService | ContainerService; mini?: boolean }>()

const TONE_CLASSES: Record<IconTone, string> = {
  node: 'bg-p-node-bg text-p-node',
  pg: 'bg-p-pg-bg text-p-pg',
  rd: 'bg-p-rd-bg text-p-rd',
  s3: 'bg-p-s3-bg text-p-s3',
  mail: 'bg-p-mail-bg text-p-mail',
  ls: 'bg-p-ls-bg text-p-ls',
  off: 'bg-p-off-bg text-p-off',
}

const icon = computed(() => iconFor(props.service))
</script>

<template>
  <span
    aria-hidden="true"
    :class="[
      'inline-flex flex-none items-center justify-center rounded-full font-mono font-semibold',
      mini ? 'h-[22px] w-[22px] text-[9px] ring-2 ring-soft' : 'h-7 w-7 text-[11px]',
      TONE_CLASSES[icon.tone],
    ]"
  >{{ icon.label }}</span>
</template>
