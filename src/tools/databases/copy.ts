// Copy formats of a result set. Values are text or null; null is NULL only in display and Markdown.

export function cellText(value: string | null): string {
  return value === null ? 'NULL' : value
}

export function toTsv(columns: string[], rows: (string | null)[][], withHeader = true): string {
  const clean = (v: string | null) => (v === null ? '' : v.replace(/[\t\n\r]/g, ' '))
  const lines = rows.map((row) => row.map(clean).join('\t'))
  if (withHeader) lines.unshift(columns.map(clean).join('\t'))
  return lines.join('\n')
}

export function toCsv(columns: string[], rows: (string | null)[][]): string {
  const quote = (v: string | null) => {
    if (v === null) return ''
    return /[,"\n\r]/.test(v) ? `"${v.replace(/"/g, '""')}"` : v
  }
  return [columns, ...rows].map((row) => row.map(quote).join(',')).join('\r\n')
}

/** Column names made unique: a repeated name gets `_2`, `_3`, … */
function uniqueKeys(columns: string[]): string[] {
  const seen = new Map<string, number>()
  const used = new Set<string>()
  return columns.map((name) => {
    let n = seen.get(name) ?? 0
    let key = name
    do {
      n += 1
      key = n === 1 ? name : `${name}_${n}`
    } while (used.has(key))
    seen.set(name, n)
    used.add(key)
    return key
  })
}

export function toJson(columns: string[], rows: (string | null)[][]): string {
  const keys = uniqueKeys(columns)
  const objects = rows.map((row) => Object.fromEntries(keys.map((key, i) => [key, row[i] ?? null])))
  return JSON.stringify(objects, null, 2)
}

export function toMarkdown(columns: string[], rows: (string | null)[][]): string {
  const clean = (v: string | null) =>
    v === null ? 'NULL' : v.replace(/\|/g, '\\|').replace(/\r\n|\r|\n/g, '<br>')
  const line = (cells: string[]) => `| ${cells.join(' | ')} |`
  return [
    line(columns.map(clean)),
    line(columns.map(() => '---')),
    ...rows.map((row) => line(row.map(clean))),
  ].join('\n')
}
