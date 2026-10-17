<script setup lang="ts">
import { computed } from 'vue'

const props = defineProps<{ fraction: number; kind: 'cpu' | 'mem' }>()

const FILL_CLASSES = { cpu: 'bg-accent', mem: 'bg-warm' } as const

const clamped = computed(() => (Number.isFinite(props.fraction) ? Math.min(1, Math.max(0, props.fraction)) : 0))
const percent = computed(() => Math.round(clamped.value * 100))
const LABELS = { cpu: 'CPU', mem: 'Memory' } as const
</script>

<template>
  <span
    role="meter"
    aria-valuemin="0"
    aria-valuemax="100"
    :aria-valuenow="percent"
    :aria-label="LABELS[kind]"
    class="inline-block h-1.5 w-14 flex-none overflow-hidden rounded-full bg-track"
  >
    <span
      data-fill
      :class="['block h-full rounded-full', FILL_CLASSES[kind]]"
      :style="{ width: `${clamped * 100}%` }"
    ></span>
  </span>
</template>
