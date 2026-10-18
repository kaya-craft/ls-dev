// Mocked Tauri backend for README screenshots: the real frontend, fake and neutral data.
// Open /scripts/screenshots/index.html?tool=services|worktrees|databases|tunnels&theme=light|dark&detail=1
// on the Vite dev server. The theme setting follows `theme`; the colours follow prefers-color-scheme.
import type { Settings } from '../../src/core/types'
import type { ContainerService, ProcessService, Snapshot } from '../../src/tools/services/types'
import type { LogChunk } from '../../src/tools/services/api'
import type { WorktreeInfo, WorktreesSnapshot } from '../../src/tools/worktrees/types'
import type { ConnectionsView, RunOutput } from '../../src/tools/databases/types'
import type { Tunnel, TunnelsStatus } from '../../src/tools/tunnels/types'
import type { Tool } from '../../src/shell/navigation'

const params = new URLSearchParams(location.search)
const TOOLS: Tool[] = ['services', 'worktrees', 'databases', 'tunnels']
const tool: Tool = TOOLS.find((t) => t === params.get('tool')) ?? 'services'
const theme = params.get('theme') === 'dark' ? 'dark' : 'light'
const detail = params.get('detail') === '1'

const NOW = Date.now()
const SEC = Math.floor(NOW / 1000)
const MB = 1024 * 1024
const GB = 1024 * MB
const HOME = '/Users/me'
const SHOP = `${HOME}/Developer/acme/shop`
const BLOG = `${HOME}/Developer/acme/blog`
const WT = `${HOME}/Developer/acme/shop.worktrees`

// ---- Services ---------------------------------------------------------------

function proc(p: Partial<ProcessService> & Pick<ProcessService, 'rootPid' | 'cwd' | 'project'>): ProcessService {
  return {
    id: `proc:${p.rootPid}:${p.cwd}`,
    kind: 'dev',
    runtime: 'node',
    ports: [],
    pid: p.rootPid + 7,
    script: 'pnpm dev',
    command: 'node pnpm dev',
    uptimeSec: 3600,
    cpuPercent: 1,
    memBytes: 100 * MB,
    tree: [],
    restart: null,
    worktree: null,
    managed: false,
    ...p,
  }
}

function container(c: Partial<ContainerService> & Pick<ContainerService, 'name' | 'image'>): ContainerService {
  const containerId = c.containerId ?? c.name.replace(/[^a-z0-9]/g, '').padEnd(12, '0').slice(0, 12)
  const service = c.name.replace(/^shop-/, '').replace(/-1$/, '')
  return {
    id: `docker:${containerId}`,
    containerId,
    state: 'running',
    status: 'Up 6 days',
    uptimeSec: 6 * 86_400,
    ports: [],
    cpuPercent: 0.3,
    memBytes: 40 * MB,
    compose: { project: 'shop', service, workingDir: SHOP },
    ...c,
  }
}

const processes: ProcessService[] = [
  proc({
    rootPid: 41208,
    cwd: `${SHOP}/packages/web`,
    project: 'shop/web',
    runtime: 'vite',
    ports: [{ port: 5173, responsive: true }],
    script: 'pnpm --filter @acme/web dev',
    command: 'node /opt/homebrew/bin/pnpm --filter @acme/web dev',
    uptimeSec: 2 * 3600 + 14 * 60,
    cpuPercent: 3.8,
    memBytes: 312 * MB,
    tree: [
      { pid: 41208, command: 'node /opt/homebrew/bin/pnpm --filter @acme/web dev', depth: 0 },
      { pid: 41215, command: 'node node_modules/.bin/vite', depth: 1 },
      { pid: 41231, command: 'esbuild --service=0.25.0 --ping', depth: 2 },
    ],
    restart: { argv: ['pnpm', '--filter', '@acme/web', 'dev'], cwd: SHOP },
    managed: true,
  }),
  proc({
    rootPid: 52114,
    cwd: `${WT}/chore-deps/packages/api`,
    project: 'shop/api',
    ports: [{ port: 3000, responsive: true }],
    script: 'pnpm --filter @acme/api dev',
    command: 'node /opt/homebrew/bin/pnpm --filter @acme/api dev',
    uptimeSec: 47 * 60,
    cpuPercent: 1.6,
    memBytes: 188 * MB,
    tree: [
      { pid: 52114, command: 'node /opt/homebrew/bin/pnpm --filter @acme/api dev', depth: 0 },
      { pid: 52121, command: 'node --watch src/server.ts', depth: 1 },
    ],
    restart: { argv: ['pnpm', '--filter', '@acme/api', 'dev'], cwd: `${WT}/chore-deps` },
    worktree: { branch: 'chore/deps', path: `${WT}/chore-deps`, mainPath: SHOP },
  }),
  proc({
    rootPid: 41290,
    cwd: `${SHOP}/packages/worker`,
    project: 'shop/worker',
    ports: [{ port: 9229, responsive: null }],
    script: 'pnpm --filter @acme/worker dev',
    command: 'node /opt/homebrew/bin/pnpm --filter @acme/worker dev',
    uptimeSec: 2 * 3600 + 13 * 60,
    cpuPercent: 0.7,
    memBytes: 142 * MB,
    tree: [
      { pid: 41290, command: 'node /opt/homebrew/bin/pnpm --filter @acme/worker dev', depth: 0 },
      { pid: 41298, command: 'node --inspect src/worker.ts', depth: 1 },
    ],
    restart: { argv: ['pnpm', '--filter', '@acme/worker', 'dev'], cwd: SHOP },
  }),
  proc({
    rootPid: 38870,
    cwd: BLOG,
    project: 'blog',
    runtime: 'nuxt',
    ports: [{ port: 3100, responsive: true }],
    script: 'pnpm dev',
    command: 'node /opt/homebrew/bin/pnpm dev',
    uptimeSec: 5 * 3600 + 2 * 60,
    cpuPercent: 2.1,
    memBytes: 264 * MB,
    tree: [
      { pid: 38870, command: 'node /opt/homebrew/bin/pnpm dev', depth: 0 },
      { pid: 38881, command: 'node node_modules/.bin/nuxi dev --port 3100', depth: 1 },
    ],
    restart: { argv: ['pnpm', 'dev'], cwd: BLOG },
  }),
]

const containers: ContainerService[] = [
  container({
    name: 'shop-postgres-1',
    containerId: '3f2a9c81d0e4',
    image: 'pgvector/pgvector:pg17',
    ports: [{ port: 5432, responsive: true }],
    cpuPercent: 0.9,
    memBytes: 96 * MB,
  }),
  container({
    name: 'shop-redis-1',
    containerId: '8b71e05c2a93',
    image: 'redis:7-alpine',
    ports: [{ port: 6379, responsive: true }],
    cpuPercent: 0.4,
    memBytes: 12 * MB,
  }),
  container({
    name: 'shop-mailpit-1',
    containerId: 'c4d90a17e6b2',
    image: 'axllent/mailpit:latest',
    ports: [{ port: 8025, responsive: true }],
    cpuPercent: 0.1,
    memBytes: 21 * MB,
  }),
  container({
    name: 'shop-minio-1',
    containerId: 'e95b3d4f7180',
    image: 'minio/minio:latest',
    state: 'exited',
    status: 'Exited (0) 2 days ago',
    uptimeSec: null,
    ports: [{ port: 9000, responsive: null }],
    cpuPercent: null,
    memBytes: null,
  }),
]

const standalone: ContainerService = {
  ...container({
    name: 'meilisearch',
    containerId: '5a0c7e2b91f4',
    image: 'getmeili/meilisearch:v1.12',
    status: 'Up 3 hours',
    uptimeSec: 3 * 3600 + 20 * 60,
    ports: [{ port: 7700, responsive: true }],
    cpuPercent: 0.2,
    memBytes: 58 * MB,
  }),
  compose: null,
}

function snapshot(takenAt: number, wobble = 0): Snapshot {
  const w = (v: number, amp: number) => Math.max(0, v + (Math.sin(wobble * 0.45 + v) * 0.7 + Math.sin(wobble * 1.3 + v * 2) * 0.3) * amp)
  return {
    takenAt,
    dockerAvailable: true,
    processes: processes.map((p) => ({ ...p, cpuPercent: w(p.cpuPercent, p.cpuPercent * 0.6), memBytes: Math.round(w(p.memBytes, 18 * MB)) })),
    compose: [
      {
        project: 'shop',
        workingDir: SHOP,
        containers: containers.map((c) =>
          c.state === 'running' ? { ...c, cpuPercent: w(c.cpuPercent ?? 0, 0.3), memBytes: Math.round(w(c.memBytes ?? 0, 3 * MB)) } : c,
        ),
      },
    ],
    standalone: [standalone],
  }
}

const LOGS = [
  'VITE v8.0.16  ready in 412 ms',
  '➜  Local:   http://localhost:5173/',
  '[vite] hmr update /src/pages/Checkout.vue',
  '[vite] hmr update /src/components/CartLine.vue',
  '[vite] page reload src/router.ts',
]

// ---- Worktrees --------------------------------------------------------------

const DAY = 86_400
function wt(name: string, w: Partial<WorktreeInfo>): WorktreeInfo {
  return {
    path: `${WT}/${name}`,
    branch: null,
    lastCommitAt: SEC - DAY,
    changes: 0,
    upstream: { kind: 'tracking', ahead: 0 },
    merged: false,
    trusted: true,
    sizeBytes: 200 * MB,
    ...w,
  }
}

const worktrees: WorktreesSnapshot = {
  takenAt: NOW,
  scanning: false,
  repos: [
    {
      path: SHOP,
      name: 'shop',
      mainSize: Math.round(1.6 * GB),
      worktrees: [
        wt('feat-checkout', { branch: 'feat/checkout', lastCommitAt: SEC - 9 * DAY, merged: true, sizeBytes: Math.round(1.2 * GB) }),
        wt('feat-search', { branch: 'feat/search', lastCommitAt: SEC - 3 * 3600, changes: 3, sizeBytes: 640 * MB }),
        wt('fix-cart', { branch: 'fix/cart', lastCommitAt: SEC - 2 * DAY, upstream: { kind: 'tracking', ahead: 2 }, sizeBytes: 420 * MB }),
        wt('chore-deps', { branch: 'chore/deps', lastCommitAt: SEC - 40 * 60, upstream: { kind: 'none' }, sizeBytes: 120 * MB }),
      ],
    },
  ],
}

// ---- Databases --------------------------------------------------------------

const DETECTED_ID = 'docker:shop-postgres-1'
const DRAFT = 'select id, email, plan, created_at from accounts order by created_at desc limit 50;'

const connections: ConnectionsView = {
  saved: [
    {
      id: 'b6c1f0a2-4e57-4d0b-9a1e-2f3c8d7e6a10',
      name: 'Acme Staging',
      env: 'staging',
      color: '#a3793e',
      host: 'db.staging.example.com',
      port: 5432,
      user: 'readonly',
      database: 'shop',
      sslMode: 'require',
      readOnly: true,
    },
  ],
  detected: [{ id: DETECTED_ID, containerId: '3f2a9c81d0e4', name: 'shop-postgres-1', project: 'shop', port: 5432, running: true }],
  dockerAvailable: true,
}

const PLANS = ['free', 'pro', 'team', 'pro', 'free', 'enterprise']
const runOutput: RunOutput = {
  statements: [{ rows: 20, returnedColumns: true }],
  result: {
    columns: ['id', 'email', 'plan', 'created_at'],
    rows: Array.from({ length: 20 }, (_, i) => {
      const n = 118 - i * 5 - (i % 2) * 2
      const day = new Date(Date.UTC(2026, 9, 17, 18, 40) - i * 7.3 * 3600_000)
      const created = day.toISOString().replace('T', ' ').replace(/\.\d+Z$/, '+00')
      return [String(1200 - i * 3), `user${n}@example.com`, i === 6 ? null : PLANS[i % PLANS.length], created]
    }),
    truncated: false,
  },
  elapsedMs: 14,
  rolledBack: false,
}

// ---- Tunnels ----------------------------------------------------------------

const tunnelsStatus: TunnelsStatus = {
  binary: '/opt/homebrew/bin/cloudflared',
  loggedIn: true,
  domain: 'dev.example.com',
  stable: [{ cwd: `${SHOP}/packages/web`, hostname: 'me-shop.dev.example.com' }],
}

const MIN = 60_000
const tunnels: Tunnel[] = [
  {
    id: 't-quick',
    kind: 'quick',
    port: 5173,
    project: 'shop/web',
    cwd: `${SHOP}/packages/web`,
    url: 'https://bright-river-falcon.trycloudflare.com',
    status: 'open',
    error: null,
    startedAt: NOW - 48 * MIN,
    expiresAt: NOW + (3 * 60 + 12) * MIN + 30_000,
  },
  {
    id: 't-stable',
    kind: 'stable',
    port: 3000,
    project: 'shop/api',
    cwd: `${WT}/chore-deps/packages/api`,
    url: 'https://me-shop.dev.example.com',
    status: 'open',
    error: null,
    startedAt: NOW - 25 * MIN,
    expiresAt: NOW + (7 * 60 + 35) * MIN,
  },
]

// ---- Settings ---------------------------------------------------------------

let settings: Settings = {
  pollIntervalMs: 3000,
  showAll: false,
  terminal: 'terminal-app',
  editorCommand: 'code',
  notifyOnStop: true,
  launchAtLogin: false,
  theme,
  worktreeRoots: [`${HOME}/Developer`],
  worktreeExcludes: [],
  tunnelDomain: 'dev.example.com',
  tunnelPrefix: 'me',
  tunnelHours: 4,
}

// ---- Tauri internals --------------------------------------------------------

type Callback = (payload: unknown) => void
const callbacks = new Map<number, Callback>()
const listeners = new Map<number, { event: string; handler: number }>()
let nextId = 1

function emit(event: string, payload: unknown) {
  for (const [id, l] of listeners) if (l.event === event) callbacks.get(l.handler)?.({ event, id, payload })
}

/** Several snapshots in a row, so the stat tiles have a CPU and memory history. */
function emitSnapshots() {
  for (let i = 0; i < 24; i++) setTimeout(() => emit('services:changed', snapshot(NOW - (24 - i) * 3000, i)), i * 5)
}

function emitLogs(id: string) {
  const chunk: LogChunk = { id, lines: LOGS.map((line) => ({ stream: 'stdout', line })) }
  setTimeout(() => emit('logs:chunk', chunk), 50)
}

const handlers: Record<string, (args: Record<string, unknown>) => unknown> = {
  get_settings: () => settings,
  set_settings: (a) => (settings = a.settings as Settings),
  get_snapshot: () => snapshot(NOW - 120_000),
  logs_open: (a) => emitLogs(a.id as string),
  worktrees_get: () => worktrees,
  databases_list: () => connections,
  databases_run: () => runOutput,
  databases_history: () => [DRAFT, 'select count(*) from orders;'],
  tunnels_status: () => tunnelsStatus,
  tunnels_list: () => tunnels,
  tunnels_output: () => ['INF Registered tunnel connection connIndex=0 location=cdg01'],
  tunnels_suggest_name: () => ({ subdomain: 'me-shop', remembered: true }),
  'plugin:event|listen': (a) => {
    const id = nextId++
    listeners.set(id, { event: a.event as string, handler: a.handler as number })
    if (a.event === 'services:changed') emitSnapshots()
    return id
  },
  'plugin:event|unlisten': (a) => void listeners.delete(a.eventId as number),
}

const w = window as unknown as Record<string, unknown>
w.__TAURI_INTERNALS__ = {
  invoke: async (cmd: string, args: Record<string, unknown> = {}) => handlers[cmd]?.(args) ?? null,
  transformCallback: (cb: Callback, once = false) => {
    const id = nextId++
    callbacks.set(id, (payload) => {
      if (once) callbacks.delete(id)
      cb(payload)
    })
    return id
  },
  unregisterCallback: (id: number) => void callbacks.delete(id),
  convertFileSrc: (path: string) => path,
  metadata: { currentWindow: { label: 'main' }, currentWebview: { windowLabel: 'main', label: 'main' } },
}
w.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: (_event: string, id: number) => void listeners.delete(id) }

// ---- Starting state ---------------------------------------------------------

localStorage.clear()
localStorage.setItem('lsdev.navigation', JSON.stringify({ currentTool: tool, sidebarCollapsed: false, servicesSection: 'all' }))
localStorage.setItem('lsdev.databases.active', DETECTED_ID)
localStorage.setItem('lsdev.databases.drafts', JSON.stringify({ [DETECTED_ID]: DRAFT }))

void import('../../src/main').then(async () => {
  if (detail && tool === 'services') {
    const { select } = await import('../../src/tools/services/store/services')
    setTimeout(() => select(processes[0].id), 200)
  }
  if (tool === 'databases') {
    const { useDatabases } = await import('../../src/tools/databases/store')
    setTimeout(() => void useDatabases().run(), 200)
  }
})
