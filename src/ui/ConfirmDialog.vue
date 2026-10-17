<script setup lang="ts">
import { nextTick, ref, watch } from 'vue'
import { answer, useConfirm } from './store/confirm'
import { BTN_DIALOG_DANGER, BTN_DIALOG } from './layout'

const state = useConfirm()
const checked = ref(false)
const confirmBtn = ref<HTMLButtonElement | null>(null)
watch(
  () => state.request,
  async (request) => {
    checked.value = false
    if (request) {
      await nextTick()
      confirmBtn.value?.focus()
    }
  },
)
</script>

<template>
  <div v-if="state.request" class="fixed inset-0 z-50 flex items-center justify-center bg-black/40">
    <div role="dialog" aria-modal="true" class="w-[520px] max-w-[90vw] rounded-[12px] border border-line bg-card p-5 shadow-card">
      <h3 class="mb-2 text-[15px] font-semibold tracking-tight">{{ state.request.title }}</h3>
      <p class="text-[12.5px] leading-relaxed text-ink-2">{{ state.request.body }}</p>
      <pre v-if="state.request.details?.length" class="mt-3 max-h-48 overflow-auto rounded-[8px] border border-line bg-soft p-2.5 font-mono text-xs">{{ state.request.details.join('\n') }}</pre>
      <label v-if="state.request.checkbox" class="mt-3 flex items-center gap-2 text-[12.5px]">
        <input v-model="checked" type="checkbox" class="accent-accent" /> {{ state.request.checkbox }}
      </label>
      <div class="mt-4 flex justify-end gap-2">
        <button :class="BTN_DIALOG" @click="answer({ ok: false, checked })">Cancel</button>
        <button
          :class="state.request.danger ? BTN_DIALOG_DANGER : BTN_DIALOG"
          ref="confirmBtn"
          @click="answer({ ok: true, checked })"
        >{{ state.request.confirmLabel }}</button>
      </div>
    </div>
  </div>
</template>
