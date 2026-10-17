// Mirrors the serde shapes of src-tauri/src/tools/tunnels (process.rs, commands.rs).
export type TunnelKind = 'quick' | 'stable'
export type TunnelStatus = 'starting' | 'open' | 'error'

export type Tunnel = {
  id: string
  kind: TunnelKind
  port: number
  project: string
  cwd: string | null
  url: string | null
  status: TunnelStatus
  error: string | null
  /** Epoch milliseconds. */
  startedAt: number
  /** Epoch milliseconds. */
  expiresAt: number
}

export type StableEntry = { cwd: string; hostname: string }

export type TunnelsStatus = {
  binary: string | null
  loggedIn: boolean
  domain: string | null
  stable: StableEntry[]
}

export type SuggestedName = { subdomain: string; remembered: boolean }

export type OpenRequest = {
  port: number
  project: string
  cwd: string | null
  kind: TunnelKind
  subdomain: string | null
  hours: 1 | 4 | 8 | 24
}
