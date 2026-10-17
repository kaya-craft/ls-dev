import { open } from '@tauri-apps/plugin-dialog'
import { worktreesApi } from './api'
import { setRemoving, setWorktreesError, useWorktrees } from './store'
import { confirm } from '../../ui/store/confirm'
import { toasts } from '../../ui/store/toasts'
import { updateSettings, useSettings } from '../../ui/store/settings'
import { formatBytes } from '../../ui/format'
import type { ProcessService, Worktree } from '../services/types'
import type { RemoveMergedReport } from './types'

function message(error: unknown): string {
  return typeof error === 'string' ? error : error instanceof Error ? error.message : String(error)
}

async function attempt(run: () => Promise<void>) {
  try {
    await run()
  } catch (error) {
    toasts.error(message(error))
  }
}

const IGNORED_FILES_NOTICE = 'Ignored files in the worktree (.env, node_modules, build output) are deleted.'

/** Marks `paths` as being removed while `run` (removal and rescan) is in progress. */
async function whileRemoving<T>(paths: string[], run: () => Promise<T>): Promise<T> {
  setRemoving(paths, true)
  try {
    return await run()
  } finally {
    setRemoving(paths, false)
  }
}

export async function openShell(path: string) {
  await attempt(() => worktreesApi.openShell(path))
}

/** Untrusted repository: the app runs no git command there; the user removes the worktree from a terminal. */
async function offerTerminal(wt: Worktree, reason: string) {
  const { ok } = await confirm({
    title: 'Remove from a terminal?',
    body: `${reason} The terminal opens in ${wt.mainPath} with the command printed; nothing runs until you press Enter.`,
    details: [`git worktree remove '${wt.path}'`],
    confirmLabel: 'Open terminal',
  })
  if (ok) await attempt(() => worktreesApi.openTerminal(wt))
}

/** Two-step removal: the second confirmation only appears when Git asks for --force. */
export async function removeWorktree(wt: Worktree, affected: ProcessService[]) {
  const first = await confirm({
    title: `Remove worktree ${wt.branch}?`,
    body: `${wt.path} will be removed with "git worktree remove". The Git branch is kept. ${IGNORED_FILES_NOTICE}`,
    details: affected.length > 0 ? ['Services stopped first:', ...affected.map((s) => `  ${s.project}`)] : undefined,
    confirmLabel: 'Remove worktree',
    danger: true,
  })
  if (!first.ok) return
  try {
    await whileRemoving([wt.path], async () => {
      await worktreesApi.removeWorktree(wt, false)
      await attempt(worktreesApi.scan)
    })
    toasts.success(`Worktree ${wt.branch} removed`)
  } catch (error) {
    const text = message(error)
    if (text.startsWith('untrusted:')) {
      await offerTerminal(wt, text.slice('untrusted:'.length))
      return
    }
    if (!text.startsWith('needs-force:')) {
      toasts.error(text)
      return
    }
    const second = await confirm({
      title: 'Force remove?',
      body: `${text.slice('needs-force:'.length)}. Uncommitted changes and untracked files in this worktree will be lost.`,
      confirmLabel: 'Force remove',
      danger: true,
    })
    if (!second.ok) return
    await attempt(() =>
      whileRemoving([wt.path], async () => {
        await worktreesApi.removeWorktree(wt, true)
        toasts.success(`Worktree ${wt.branch} removed`)
        await worktreesApi.scan()
      }),
    )
  }
}

const SKIPPED_SHOWN = 5

function plural(n: number): string {
  return `${n} worktree${n === 1 ? '' : 's'}`
}

function skippedLines(skipped: RemoveMergedReport['skipped']): string[] {
  if (skipped.length === 0) return []
  const lines = skipped.slice(0, SKIPPED_SHOWN).map((s) => `${s.path}: ${s.reason}`)
  if (skipped.length > SKIPPED_SHOWN) lines.push(`+${skipped.length - SKIPPED_SHOWN} more`)
  return [`Skipped ${skipped.length}:`, ...lines]
}

export async function removeAllMerged() {
  const eligible = useWorktrees().eligibleMerged.value
  if (eligible.length === 0) {
    toasts.success('No merged worktree to remove')
    return
  }
  const freed = eligible.reduce((n, w) => n + (w.sizeBytes ?? 0), 0)
  const unknownSizes = eligible.some((w) => w.sizeBytes === null)
  const { ok } = await confirm({
    title: `Remove ${eligible.length} merged worktree${eligible.length === 1 ? '' : 's'}?`,
    body: `${formatBytes(freed)}${unknownSizes ? ' (+ unknown sizes)' : ''} will be freed. Each worktree is removed with "git worktree remove"; Git branches are kept. ${IGNORED_FILES_NOTICE}`,
    details: eligible.map((w) => `${w.branch ?? '(detached)'}  ${w.path}  ${formatBytes(w.sizeBytes)}`),
    confirmLabel: 'Remove all',
    danger: true,
  })
  if (!ok) return
  const paths = eligible.map((w) => w.path)
  try {
    const report = await whileRemoving(paths, async () => {
      const report = await worktreesApi.removeMerged(paths)
      await attempt(worktreesApi.scan)
      return report
    })
    const skipped = skippedLines(report.skipped)
    if (report.removed.length > 0) {
      toasts.success([`Removed ${plural(report.removed.length)}`, ...skipped].join('\n'))
    } else {
      toasts.error(['No worktree removed', ...skipped].join('\n'))
    }
  } catch (error) {
    toasts.error(message(error))
  }
}

export async function addFolder() {
  await attempt(async () => {
    const dir = await open({ directory: true })
    if (typeof dir !== 'string') return
    const roots = useSettings().settings?.worktreeRoots ?? []
    if (roots.includes(dir)) return
    await updateSettings({ worktreeRoots: [...roots, dir] })
    await worktreesApi.scan()
  })
}

export async function excludeRepo(path: string) {
  await attempt(async () => {
    const excludes = useSettings().settings?.worktreeExcludes ?? []
    if (excludes.includes(path)) return
    await updateSettings({ worktreeExcludes: [...excludes, path] })
    await worktreesApi.scan()
  })
}

/** Manual rescan toasts on failure; a silent one (interval) only records the error in the store. */
export async function rescan(options: { silent?: boolean } = {}) {
  try {
    await worktreesApi.scan()
  } catch (error) {
    if (options.silent) setWorktreesError(message(error))
    else toasts.error(message(error))
  }
}
