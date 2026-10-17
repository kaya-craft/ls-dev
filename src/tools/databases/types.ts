// Mirrors the serde JSON of src-tauri/src/tools/databases (camelCase; RunError is tagged by `kind`).

export type SslMode = 'disable' | 'prefer' | 'require'

export interface SavedConnection {
  id: string
  name: string
  env: string | null
  color: string | null
  host: string
  port: number
  user: string
  database: string
  sslMode: SslMode
  readOnly: boolean
}

/** What the connection dialog sends; `id: null` means a new connection. */
export interface ConnectionDraft {
  id: string | null
  name: string
  env: string | null
  color: string | null
  host: string
  port: number
  user: string
  database: string
  sslMode: SslMode
  readOnly: boolean
}

export interface DetectedConnection {
  id: string
  containerId: string
  name: string
  project: string | null
  port: number | null
  running: boolean
}

export interface ConnectionsView {
  saved: SavedConnection[]
  detected: DetectedConnection[]
  dockerAvailable: boolean
}

export interface ParsedUrl {
  name: string | null
  env: string | null
  color: string | null
  host: string
  port: number
  user: string
  password: string | null
  database: string
  sslMode: SslMode
}

export interface StatementSummary {
  rows: number
  returnedColumns: boolean
}

export interface ResultSet {
  columns: string[]
  rows: (string | null)[][]
  truncated: boolean
}

export interface RunOutput {
  statements: StatementSummary[]
  result: ResultSet | null
  elapsedMs: number
  /** True when stopping at the row limit undid earlier work of the same run. */
  rolledBack: boolean
}

export type RunError =
  | { kind: 'connect'; host: string; port: number; cause: string }
  | { kind: 'closed' }
  | { kind: 'db'; message: string; code: string; hint: string | null; position: number | null }
  | { kind: 'cancelled' }
  | { kind: 'other'; message: string }
