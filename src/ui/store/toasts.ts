import { reactive } from 'vue'

export type Toast = { id: number; kind: 'success' | 'error'; message: string }

const state = reactive({ items: [] as Toast[] })
let nextId = 1

function push(kind: Toast['kind'], message: string, ttl: number) {
  const id = nextId++
  state.items.push({ id, kind, message })
  setTimeout(() => dismiss(id), ttl)
}

export function dismiss(id: number) {
  const i = state.items.findIndex((t) => t.id === id)
  if (i >= 0) state.items.splice(i, 1)
}

export const toasts = {
  success: (message: string) => push('success', message, 4000),
  error: (message: string) => push('error', message, 8000),
}

export function useToasts() {
  return state
}
