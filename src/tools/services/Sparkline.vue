<script setup lang="ts">
import { computed } from 'vue'

const props = defineProps<{ series: number[] }>()

const W = 78
const H = 30
const PAD = 3

// Fewer than two points, or a constant series: a flat line at mid height.
const points = computed(() => {
  const s = props.series
  if (s.length < 2) return `0,${H / 2} ${W},${H / 2}`
  const min = Math.min(...s)
  const max = Math.max(...s)
  const span = max - min
  return s
    .map((v, i) => {
      const x = Math.round((i / (s.length - 1)) * W * 10) / 10
      const y = span === 0 ? H / 2 : Math.round((H - PAD - ((v - min) / span) * (H - 2 * PAD)) * 10) / 10
      return `${x},${y}`
    })
    .join(' ')
})
</script>

<template>
  <svg :width="W" :height="H" :viewBox="`0 0 ${W} ${H}`" aria-hidden="true" class="shrink-0 overflow-visible">
    <polyline
      :points="points"
      fill="none"
      stroke="currentColor"
      stroke-width="2"
      stroke-linejoin="round"
      stroke-linecap="round"
    />
  </svg>
</template>
