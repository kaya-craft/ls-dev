import type { ServicesSection } from '../../shell/navigation'
import type { ComposeGroup, ContainerService, ProcessService } from './types'

export type SectionKey = string

export type SectionStats = {
  ports: number
  responding: number
  running: number
  stopped: number
  cpu: number | null
  mem: number | null
  isCompose: boolean
}

export type SectionHistory = { cpu: number[]; mem: number[] }

export function sectionKey(section: ServicesSection): SectionKey {
  return typeof section === 'string' ? section : `compose:${section.compose}`
}

function sum(values: Array<number | null>): number | null {
  const known = values.filter((v): v is number => v !== null)
  return known.length === 0 ? null : known.reduce((a, b) => a + b, 0)
}

export function sectionStats(
  view: { processes: ProcessService[]; compose: ComposeGroup[]; standalone: ContainerService[] },
  isCompose: boolean,
): SectionStats {
  const containers = [...view.compose.flatMap((g) => g.containers), ...view.standalone]
  const runningContainers = containers.filter((c) => c.state === 'running')
  const live = [...view.processes, ...runningContainers]
  const ports = live.flatMap((s) => s.ports)
  return {
    ports: ports.length,
    responding: ports.filter((p) => p.responsive === true).length,
    running: live.length,
    stopped: containers.length - runningContainers.length,
    cpu: sum(live.map((s) => s.cpuPercent)),
    mem: sum(live.map((s) => s.memBytes)),
    isCompose,
  }
}

export function pushHistory(
  history: Record<string, SectionHistory>,
  key: SectionKey,
  stats: SectionStats,
  max = 30,
): Record<string, SectionHistory> {
  const entry = (history[key] ??= { cpu: [], mem: [] })
  // An unknown value is not a zero: skip the point instead of drawing a dip.
  if (stats.cpu !== null) entry.cpu = [...entry.cpu, stats.cpu].slice(-max)
  if (stats.mem !== null) entry.mem = [...entry.mem, stats.mem].slice(-max)
  return history
}
