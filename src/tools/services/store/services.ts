import { computed, reactive } from 'vue'
import { sinkStopped, sortServices } from '../sort'
import { useView } from './view'
import { useNavigation, type ServicesSection } from '../../../shell/navigation'
import { pushHistory, sectionKey, sectionStats, type SectionHistory, type SectionStats } from '../stats'
import type { ComposeGroup, ContainerService, ProcessService, Service, Snapshot } from '../types'

const state = reactive({
  snapshot: null as Snapshot | null,
  filter: '',
  selectedId: null as string | null,
  error: null as string | null,
  history: {} as Record<string, SectionHistory>,
})

type View = { processes: ProcessService[]; compose: ComposeGroup[]; standalone: ContainerService[] }

function pickSection(view: View, section: ServicesSection): View {
  if (section === 'all') return view
  if (section === 'dev') return { ...view, compose: [], standalone: [] }
  if (section === 'standalone') return { ...view, processes: [], compose: [] }
  return {
    ...view,
    processes: [],
    standalone: [],
    compose: view.compose.filter((g) => g.project === section.compose),
  }
}

let lastRecordedAt = 0

function recordHistory(snapshot: Snapshot) {
  if (snapshot.takenAt <= 0 || snapshot.takenAt === lastRecordedAt) return
  lastRecordedAt = snapshot.takenAt
  const sections: ServicesSection[] = ['all', 'dev', 'standalone', ...snapshot.compose.map((g) => ({ compose: g.project }))]
  const keep = new Set<string>()
  for (const section of sections) {
    const key = sectionKey(section)
    keep.add(key)
    pushHistory(state.history, key, sectionStats(pickSection(snapshot, section), typeof section !== 'string'))
  }
  for (const key of Object.keys(state.history)) if (!keep.has(key)) delete state.history[key]
}

export function applySnapshot(snapshot: Snapshot) {
  state.snapshot = snapshot
  state.error = null
  recordHistory(snapshot)
}

/** Applies the startup snapshot unless a newer one already arrived through the event. */
export function applyInitialSnapshot(snapshot: Snapshot) {
  if (state.snapshot && state.snapshot.takenAt > snapshot.takenAt) return
  applySnapshot(snapshot)
}

export function setError(message: string) {
  state.error = message
}

export function setFilter(value: string) {
  state.filter = value
}

export function select(id: string | null) {
  state.selectedId = id
}

export function resetServices() {
  state.snapshot = null
  state.filter = ''
  state.selectedId = null
  state.error = null
  state.history = {}
  lastRecordedAt = 0
}

function matches(needle: string, fields: Array<string | number | null | undefined>): boolean {
  return fields.some((f) => f !== null && f !== undefined && String(f).toLowerCase().includes(needle))
}

function processMatches(p: ProcessService, needle: string) {
  return matches(needle, [p.project, p.script, p.runtime, p.worktree?.branch, ...p.ports.map((x) => x.port)])
}

function containerMatches(c: ContainerService, needle: string) {
  return matches(needle, [c.name, c.image, c.compose?.service, ...c.ports.map((x) => x.port)])
}

/** The stored section, or 'all' when it names a compose project absent from the snapshot. */
const effectiveSection = computed<ServicesSection>(() => {
  const section = useNavigation().state.servicesSection
  if (typeof section === 'string') return section
  const known = state.snapshot?.compose.some((g) => g.project === section.compose) ?? false
  return known ? section : 'all'
})

const sections = computed(() => {
  const snapshot = state.snapshot
  const needle = state.filter.trim().toLowerCase()
  if (!snapshot) return { processes: [], compose: [] as ComposeGroup[], standalone: [] }
  const sort = useView().state.sort
  let processes = snapshot.processes
  let compose = snapshot.compose
  let standalone = snapshot.standalone
  if (needle) {
    processes = processes.filter((p) => processMatches(p, needle))
    compose = compose
      .map((g) => ({
        ...g,
        containers: g.project.toLowerCase().includes(needle)
          ? g.containers
          : g.containers.filter((c) => containerMatches(c, needle)),
      }))
      .filter((g) => g.containers.length > 0)
    standalone = standalone.filter((c) => containerMatches(c, needle))
  }
  const filtered = {
    processes: sinkStopped(sortServices(processes, sort)),
    compose: compose.map((g) => ({ ...g, containers: sinkStopped(sortServices(g.containers, sort)) })),
    standalone: sinkStopped(sortServices(standalone, sort)),
  }
  return pickSection(filtered, effectiveSection.value)
})

const allServices = computed<Service[]>(() => {
  const s = state.snapshot
  if (!s) return []
  return [...s.processes, ...s.compose.flatMap((g) => g.containers), ...s.standalone]
})

const selected = computed<Service | null>(
  () => allServices.value.find((s) => s.id === state.selectedId) ?? null,
)

const listeningCount = computed(() => {
  const s = state.snapshot
  if (!s) return 0
  const running = [...s.compose.flatMap((g) => g.containers), ...s.standalone].filter((c) => c.state === 'running')
  return s.processes.reduce((n, p) => n + p.ports.length, 0) + running.reduce((n, c) => n + c.ports.length, 0)
})

const stats = computed<SectionStats>(() => {
  const section = effectiveSection.value
  const view = state.snapshot ?? { processes: [], compose: [], standalone: [] }
  return sectionStats(pickSection(view, section), typeof section !== 'string')
})

const history = computed<SectionHistory>(
  () => state.history[sectionKey(effectiveSection.value)] ?? { cpu: [], mem: [] },
)

const loading = computed(() => !state.snapshot || state.snapshot.takenAt === 0)

export function useServices() {
  return { state, sections, effectiveSection, selected, loading, listeningCount, stats, history }
}
