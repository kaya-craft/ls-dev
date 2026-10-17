import { reactive } from 'vue'
import { tunnelsApi } from './api'
import type { Tunnel, TunnelsStatus } from './types'

const state = reactive({
  tunnels: [] as Tunnel[],
  status: null as TunnelsStatus | null,
  share: { open: false, port: null as number | null, project: '', cwd: null as string | null },
})

/** Opens the share dialog; without a port the dialog lists the listening ports. */
export function openShare(target: { port: number; project: string; cwd: string | null } | null = null) {
  state.share = { open: true, port: target?.port ?? null, project: target?.project ?? '', cwd: target?.cwd ?? null }
}

export function closeShare() {
  state.share.open = false
}

export function applyTunnels(list: Tunnel[]) {
  state.tunnels = list
}

export function resetTunnels() {
  state.tunnels = []
  state.status = null
  state.share = { open: false, port: null, project: '', cwd: null }
}

/** Reloads the list and the cloudflared status. A failed call keeps the previous value. */
export async function refresh() {
  const [list, status] = await Promise.allSettled([tunnelsApi.list(), tunnelsApi.status()])
  if (list.status === 'fulfilled') applyTunnels(list.value)
  if (status.status === 'fulfilled') state.status = status.value
  const failed = [list, status].find((r) => r.status === 'rejected')
  if (failed) throw (failed as PromiseRejectedResult).reason
}

/** Reloads the cloudflared status only. A failed call keeps the previous value. */
export async function refreshStatus() {
  try {
    const status = await tunnelsApi.status()
    if (status) state.status = status
  } catch {
    // Keep the previous status.
  }
}

export const BAD_DOMAIN = 'Set a valid tunnel domain in Settings (for example dev.example.com).'

/** Same rule as validate_domain in commands.rs: labels of [a-z0-9-], none empty, no leading dash. */
export function validDomain(domain: string): boolean {
  return domain !== '' && !domain.startsWith('-') && domain.split('.').every((label) => /^[a-z0-9-]+$/.test(label))
}

type ListeningProcess = { cwd: string | null; project: string; ports: { port: number }[] }

/**
 * "Port <port> now serves <project>" when the process listening on the
 * tunnel's port runs in another folder than the one shared. Containers have
 * no folder and are not checked.
 */
export function movedNotice(t: Tunnel, processes: ListeningProcess[]): string | null {
  if (t.status === 'error' || !t.cwd) return null
  const p = processes.find((x) => x.ports.some((y) => y.port === t.port))
  if (!p?.cwd || p.cwd === t.cwd) return null
  return `Port ${t.port} now serves ${p.project}`
}

/**
 * The tunnel to show for a port: an open one, else a starting one, else (only
 * errors left) the most recently started error entry.
 */
export function forPort(port: number): Tunnel | undefined {
  const mine = state.tunnels.filter((t) => t.port === port)
  return (
    mine.find((t) => t.status === 'open') ??
    mine.find((t) => t.status === 'starting') ??
    mine.filter((t) => t.status === 'error').sort((a, b) => b.startedAt - a.startedAt)[0]
  )
}

/** "3 h 12 min left", "12 min left", "less than 1 min left". */
export function timeLeft(t: Tunnel, nowMs: number): string {
  const minutes = Math.floor((t.expiresAt - nowMs) / 60_000)
  if (minutes < 1) return 'less than 1 min left'
  const h = Math.floor(minutes / 60)
  const m = minutes % 60
  return h > 0 ? `${h} h ${m} min left` : `${m} min left`
}

export function useTunnels() {
  return { state, refresh, refreshStatus, forPort, timeLeft }
}
