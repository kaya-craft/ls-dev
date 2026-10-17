export function formatUptime(sec: number | null): string {
  if (sec === null) return '—'
  const d = Math.floor(sec / 86400)
  const h = Math.floor((sec % 86400) / 3600)
  const m = Math.floor((sec % 3600) / 60)
  if (d > 0) return `${d}d${h}h`
  if (h > 0) return `${h}h${m}m`
  if (m > 0) return `${m}m`
  return `${sec}s`
}

export function formatBytes(bytes: number | null): string {
  if (bytes === null) return '—'
  const units = ['B', 'KB', 'MB', 'GB', 'TB']
  let value = bytes
  let unit = 0
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024
    unit++
  }
  const digits = unit >= 3 ? 1 : 0
  return `${value.toFixed(digits)} ${units[unit]}`
}

export function formatCpu(percent: number | null): string {
  return percent === null ? '—' : `${percent.toFixed(1)}%`
}

/** Relative age from an elapsed duration in seconds ("3 days ago"). */
export function formatAgo(elapsedSec: number | null): string {
  if (elapsedSec === null) return '—'
  const plural = (n: number, unit: string) => `${n} ${unit}${n === 1 ? '' : 's'} ago`
  if (elapsedSec < 60) return 'just now'
  if (elapsedSec < 3600) return `${Math.floor(elapsedSec / 60)} min ago`
  if (elapsedSec < 86400) return plural(Math.floor(elapsedSec / 3600), 'hour')
  const days = Math.floor(elapsedSec / 86400)
  if (days < 30) return plural(days, 'day')
  if (days < 365) return plural(Math.floor(days / 30), 'month')
  return plural(Math.floor(days / 365), 'year')
}
