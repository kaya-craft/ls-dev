import { reactive } from 'vue'

export type ConfirmRequest = {
  title: string
  body: string
  details?: string[]
  confirmLabel: string
  danger?: boolean
  /** Label of an optional checkbox; its value comes back as `checked`. */
  checkbox?: string
}
export type ConfirmResult = { ok: boolean; checked: boolean }

const state = reactive({
  request: null as ConfirmRequest | null,
  resolve: null as ((result: ConfirmResult) => void) | null,
})

export function confirm(request: ConfirmRequest): Promise<ConfirmResult> {
  state.resolve?.({ ok: false, checked: false })
  return new Promise((resolve) => {
    state.request = request
    state.resolve = resolve
  })
}

export function answer(result: ConfirmResult) {
  state.resolve?.(result)
  state.request = null
  state.resolve = null
}

export function useConfirm() {
  return state
}
