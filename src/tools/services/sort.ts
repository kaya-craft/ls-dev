import { isProcess, type ContainerService, type ProcessService } from './types'

export type SortKey = 'uptime' | 'cpu' | 'memory'
export type SortDir = 'desc' | 'asc'
export type SortState = { key: SortKey; dir: SortDir } | null

/** Header click: new key -> desc, same key desc -> asc, same key asc -> off. */
export function nextSort(current: SortState, key: SortKey): SortState {
  if (!current || current.key !== key) return { key, dir: 'desc' }
  return current.dir === 'desc' ? { key, dir: 'asc' } : null
}

function valueOf(service: ProcessService | ContainerService, key: SortKey): number | null {
  const v = key === 'uptime' ? service.uptimeSec : key === 'cpu' ? service.cpuPercent : service.memBytes
  // Anything but a number counts as missing (null, or undefined from a stale backend).
  return typeof v === 'number' ? v : null
}

/** Returns a new array. Null values go last in both directions; ties keep input order. */
export function sortServices<T extends ProcessService | ContainerService>(list: T[], sort: SortState): T[] {
  if (!sort) return [...list]
  const sign = sort.dir === 'desc' ? -1 : 1
  return list
    .map((item, index) => ({ item, index, value: valueOf(item, sort.key) }))
    .sort((a, b) => {
      if (a.value === null && b.value === null) return a.index - b.index
      if (a.value === null) return 1
      if (b.value === null) return -1
      return sign * (a.value - b.value) || a.index - b.index
    })
    .map((x) => x.item)
}

/** Stable partition: running services first, stopped containers after. Returns a new array. */
export function sinkStopped<T extends ProcessService | ContainerService>(list: T[]): T[] {
  const isStopped = (s: T) => !isProcess(s) && s.state !== 'running'
  return [...list.filter((s) => !isStopped(s)), ...list.filter(isStopped)]
}
