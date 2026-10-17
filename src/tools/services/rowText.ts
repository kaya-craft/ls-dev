import { formatUptime } from '../../ui/format'
import { isProcess, type ContainerService, type ProcessService } from './types'

export type IconTone = 'node' | 'pg' | 'rd' | 's3' | 'mail' | 'ls' | 'off'

const RUNTIME_LABELS: Record<string, string> = { node: 'N', bun: 'B', deno: 'D' }

const IMAGE_RULES: Array<{ match: string[]; label: string; tone: IconTone }> = [
  { match: ['postgres', 'pgvector'], label: 'Pg', tone: 'pg' },
  { match: ['redis', 'valkey'], label: 'Rd', tone: 'rd' },
  { match: ['minio', 'rustfs', 's3'], label: 'S3', tone: 's3' },
  { match: ['mailpit', 'mailhog'], label: '@', tone: 'mail' },
  { match: ['localstack'], label: 'Ls', tone: 'ls' },
]

export function iconFor(s: ProcessService | ContainerService): { label: string; tone: IconTone } {
  if (isProcess(s)) {
    const known = RUNTIME_LABELS[s.runtime.toLowerCase()]
    const label = known ?? s.runtime.slice(0, 2)
    return { label: label.charAt(0).toUpperCase() + label.slice(1), tone: known ? 'node' : 'off' }
  }
  const image = s.image.toLowerCase()
  const rule = IMAGE_RULES.find((r) => r.match.some((m) => image.includes(m)))
  if (rule) return { label: rule.label, tone: s.state === 'running' ? rule.tone : 'off' }
  const initials = s.name.slice(0, 2)
  return { label: initials.charAt(0).toUpperCase() + initials.slice(1), tone: 'off' }
}

export function meterScale(kind: 'cpu' | 'mem', values: Array<number | null>): number {
  const max = Math.max(0, ...values.filter((v): v is number => v !== null))
  return Math.max(kind === 'cpu' ? 10 : 640 * 1024 * 1024, max)
}

export function meterFraction(value: number | null, scale: number): number {
  if (value === null || !Number.isFinite(value) || !Number.isFinite(scale) || value <= 0 || scale <= 0) return 0
  return Math.min(1, value / scale)
}

export function splitName(project: string): { prefix: string; last: string } {
  const i = project.lastIndexOf('/')
  return i < 0 ? { prefix: '', last: project } : { prefix: project.slice(0, i + 1), last: project.slice(i + 1) }
}

export function subline(s: ProcessService | ContainerService, opts: { narrow: boolean }): string {
  let text: string
  let running = true
  if (isProcess(s)) {
    text = `${s.script} · ${s.rootPid}`
  } else if (s.state === 'running') {
    text = s.image
  } else {
    running = false
    text = `${s.image} · ${s.status}`
  }
  if (opts.narrow && running && s.uptimeSec !== null) text += ` · ${formatUptime(s.uptimeSec)}`
  return text
}
