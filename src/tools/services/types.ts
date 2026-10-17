export type Port = { port: number; responsive: boolean | null }
export type TreeNode = { pid: number; command: string; depth: number }
export type Worktree = { branch: string; path: string; mainPath: string }
export type RestartSpec = { argv: string[]; cwd: string }

export type ProcessService = {
  id: string
  kind: 'dev' | 'other'
  runtime: string
  ports: Port[]
  pid: number
  rootPid: number
  cwd: string | null
  project: string
  script: string
  command: string
  uptimeSec: number | null
  cpuPercent: number
  memBytes: number
  tree: TreeNode[]
  restart: RestartSpec | null
  worktree: Worktree | null
  managed: boolean
}

export type ComposeInfo = { project: string; service: string; workingDir: string | null }

export type ContainerService = {
  id: string
  containerId: string
  name: string
  image: string
  state: 'running' | 'exited' | 'paused' | 'restarting' | 'created' | 'dead' | string
  status: string
  uptimeSec: number | null
  ports: Port[]
  cpuPercent: number | null
  memBytes: number | null
  compose: ComposeInfo | null
}

export type ComposeGroup = { project: string; workingDir: string | null; containers: ContainerService[] }

export type Snapshot = {
  takenAt: number
  dockerAvailable: boolean
  processes: ProcessService[]
  compose: ComposeGroup[]
  standalone: ContainerService[]
}

export type Service = ProcessService | ContainerService

export function isProcess(service: Service): service is ProcessService {
  return service.id.startsWith('proc:')
}
