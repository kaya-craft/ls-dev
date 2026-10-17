import type { ContainerService, ProcessService, Snapshot } from '../tools/services/types'

export function proc(overrides: Partial<ProcessService> = {}): ProcessService {
  return {
    id: 'proc:29414:/w/shop/packages/backend',
    kind: 'dev',
    runtime: 'node',
    ports: [{ port: 4000, responsive: true }],
    pid: 46048,
    rootPid: 29414,
    cwd: '/w/shop/packages/backend',
    project: 'shop/packages/backend',
    script: 'pnpm --filter @acme/backend dev',
    command: 'node /x/pnpm --filter @acme/backend dev',
    uptimeSec: 3600,
    cpuPercent: 1.5,
    memBytes: 200 * 1024 * 1024,
    tree: [
      { pid: 29414, command: 'node /x/pnpm --filter @acme/backend dev', depth: 0 },
      { pid: 46048, command: 'node src/server.ts', depth: 1 },
    ],
    restart: { argv: ['pnpm', '--filter', '@acme/backend', 'dev'], cwd: '/w/shop' },
    worktree: null,
    managed: false,
    ...overrides,
  }
}

export function ctr(overrides: Partial<ContainerService> = {}): ContainerService {
  return {
    id: 'docker:aaa',
    containerId: 'aaa',
    name: 'acme-shop-postgres-1',
    image: 'pgvector/pgvector:pg17',
    state: 'running',
    status: 'Up 7 weeks',
    uptimeSec: 7 * 604_800,
    ports: [{ port: 5432, responsive: true }],
    cpuPercent: 0.4,
    memBytes: 80 * 1024 * 1024,
    compose: { project: 'acme-shop', service: 'postgres', workingDir: '/w/shop/infra' },
    ...overrides,
  }
}

export function snapshot(): Snapshot {
  return {
    takenAt: 1_800_000_000_000,
    dockerAvailable: true,
    processes: [
      proc(),
      proc({
        id: 'proc:1:/w/shop-feat/packages/app',
        ports: [{ port: 3000, responsive: false }],
        project: 'shop-feat/packages/app',
        script: 'pnpm dev:app',
        worktree: { branch: 'feat/login', path: '/w/shop-feat', mainPath: '/w/shop' },
      }),
    ],
    compose: [
      {
        project: 'acme-shop',
        workingDir: '/w/shop/infra',
        containers: [
          ctr(),
          ctr({
            id: 'docker:bbb',
            containerId: 'bbb',
            name: 'acme-shop-redis-1',
            image: 'redis:7-alpine',
            ports: [{ port: 6379, responsive: true }],
            compose: { project: 'acme-shop', service: 'redis', workingDir: '/w/shop/infra' },
          }),
        ],
      },
    ],
    standalone: [
      ctr({ id: 'docker:ccc', containerId: 'ccc', name: 'legacy-mysql', image: 'mysql:8', state: 'exited', status: 'Exited (0) 3 days ago', uptimeSec: null, ports: [], cpuPercent: null, memBytes: null, compose: null }),
    ],
  }
}
