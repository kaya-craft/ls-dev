<script setup lang="ts">
import { openUrl } from '@tauri-apps/plugin-opener'
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import PageHeader from '../../ui/PageHeader.vue'
import { BTN_PAGE, BTN_PRIMARY, BTN_ROW, BTN_ROW_DANGER } from '../../ui/layout'
import { confirm } from '../../ui/store/confirm'
import { toasts } from '../../ui/store/toasts'
import { useServices } from '../services/store/services'
import { tunnelsApi } from './api'
import { BAD_DOMAIN, movedNotice, openShare, refresh, refreshStatus, useTunnels, validDomain } from './store'
import type { StableEntry, Tunnel } from './types'

defineEmits<{ 'open-settings': [] }>()

const { state, timeLeft } = useTunnels()
const { state: services } = useServices()

const COUNTDOWN_MS = 30_000
const INSTALL_COMMAND = 'brew install cloudflared'
const BADGE = 'whitespace-nowrap rounded px-1.5 font-mono text-[11px]'

const now = ref(Date.now())
let timer: ReturnType<typeof setInterval> | undefined

onMounted(() => {
  timer = setInterval(() => (now.value = Date.now()), COUNTDOWN_MS)
  window.addEventListener('focus', refreshStatus)
  void refreshStatus()
})
onBeforeUnmount(() => {
  clearInterval(timer)
  window.removeEventListener('focus', refreshStatus)
})

const message = (e: unknown) => (e instanceof Error ? e.message : String(e))

/** Ports listening in the Services snapshot (processes and running containers); null before the first snapshot. */
const listeningPorts = computed<Set<number> | null>(() => {
  const snap = services.snapshot
  if (!snap) return null
  const ports = new Set<number>()
  for (const p of snap.processes) for (const x of p.ports) ports.add(x.port)
  for (const c of [...snap.standalone, ...snap.compose.flatMap((g) => g.containers)]) {
    if (c.state === 'running') for (const x of c.ports) ports.add(x.port)
  }
  return ports
})
const notListening = (t: Tunnel) => t.status !== 'error' && listeningPorts.value !== null && !listeningPorts.value.has(t.port)
const moved = (t: Tunnel) => movedNotice(t, services.snapshot?.processes ?? [])

const setup = computed<'binary' | 'login' | 'domain' | 'bad-domain' | null>(() => {
  const s = state.status
  if (!s) return null
  if (!s.binary) return 'binary'
  if (!s.loggedIn) return 'login'
  if (!s.domain) return 'domain'
  if (!validDomain(s.domain)) return 'bad-domain'
  return null
})
const stable = computed<StableEntry[]>(() => state.status?.stable ?? [])
const folderName = (cwd: string) => cwd.split('/').filter(Boolean).pop() ?? cwd

async function copy(text: string, done: string) {
  try {
    await navigator.clipboard.writeText(text)
    toasts.success(done)
  } catch (e) {
    toasts.error(message(e))
  }
}

async function open(url: string) {
  try {
    await openUrl(url)
  } catch (e) {
    toasts.error(message(e))
  }
}

async function run(action: () => Promise<unknown>) {
  try {
    await action()
  } catch (e) {
    toasts.error(message(e))
  }
}
const extend = (t: Tunnel) => run(() => tunnelsApi.extend(t.id))
const stop = (t: Tunnel) => run(() => tunnelsApi.stop(t.id))
const connect = () => run(() => tunnelsApi.login())

const outputs = ref<Record<string, string[] | null>>({})
async function toggleOutput(t: Tunnel) {
  if (outputs.value[t.id]) {
    outputs.value = { ...outputs.value, [t.id]: null }
    return
  }
  try {
    outputs.value = { ...outputs.value, [t.id]: await tunnelsApi.output(t.id) }
  } catch (e) {
    toasts.error(message(e))
  }
}

async function deleteStable(entry: StableEntry) {
  const { ok } = await confirm({
    title: 'Delete stable URL',
    body: `The DNS record ${entry.hostname} stays in Cloudflare; delete it from the Cloudflare dashboard.`,
    details: [entry.hostname, entry.cwd],
    confirmLabel: 'Delete',
    danger: true,
  })
  if (!ok) return
  try {
    await tunnelsApi.deleteStable(entry.cwd)
    await refresh()
  } catch (e) {
    toasts.error(message(e))
  }
}
</script>

<template>
  <div class="flex min-h-0 flex-1 flex-col">
    <PageHeader title="Tunnels" subtitle="Public URLs for local servers" @open-settings="$emit('open-settings')">
      <template #action>
        <button type="button" data-testid="header-share" :class="BTN_PRIMARY" @click="openShare()">Share a port</button>
      </template>
    </PageHeader>

    <div class="min-h-0 flex-1 overflow-auto px-4 pb-4 text-[12.5px]">
      <section v-if="setup" data-testid="setup-card" class="mb-4 rounded-tile border border-line bg-card p-4">
        <template v-if="setup === 'binary'">
          <h2 class="text-[13px] font-medium">cloudflared is not installed</h2>
          <p class="mt-1 text-ink-2">Install it to open public URLs, then reopen this page.</p>
          <div class="mt-3 flex items-center gap-2">
            <code data-testid="install-command" class="rounded-[8px] bg-soft px-2.5 py-1 font-mono text-xs">{{ INSTALL_COMMAND }}</code>
            <button type="button" :class="BTN_ROW" @click="copy(INSTALL_COMMAND, 'Command copied')">Copy</button>
          </div>
        </template>
        <template v-else-if="setup === 'login'">
          <h2 class="text-[13px] font-medium">Connect cloudflared to a Cloudflare account</h2>
          <p class="mt-1 text-ink-2">Stable URLs need a Cloudflare account. Random URLs work without one.</p>
          <button type="button" :class="[BTN_PAGE, 'mt-3']" @click="connect">Connect</button>
        </template>
        <template v-else-if="setup === 'domain'">
          <h2 class="text-[13px] font-medium">No tunnel domain</h2>
          <p class="mt-1 text-ink-2">Set a tunnel domain in Settings to create stable URLs.</p>
          <button type="button" :class="[BTN_PAGE, 'mt-3']" @click="$emit('open-settings')">Open Settings</button>
        </template>
        <template v-else>
          <h2 class="text-[13px] font-medium">Invalid tunnel domain</h2>
          <p data-testid="bad-domain" class="mt-1 text-ink-2">{{ BAD_DOMAIN }}</p>
          <button type="button" :class="[BTN_PAGE, 'mt-3']" @click="$emit('open-settings')">Open Settings</button>
        </template>
      </section>

      <section v-if="state.tunnels.length > 0" aria-label="Open tunnels" class="mb-5">
        <h2 class="mb-2 text-[11.5px] font-medium uppercase tracking-wide text-ink-3">Open tunnels</h2>
        <ul class="flex flex-col gap-2">
          <li v-for="t in state.tunnels" :key="t.id" data-testid="tunnel-row" class="rounded-tile border border-line bg-card px-4 py-3">
            <div class="flex items-center gap-3">
              <div class="min-w-0 flex-1">
                <div class="flex items-center gap-2">
                  <span class="truncate font-medium">{{ t.project }}</span>
                  <span class="font-mono text-xs text-ink-2">:{{ t.port }}</span>
                  <span data-testid="kind-badge" :class="[BADGE, 'bg-soft text-ink-2']">{{ t.kind === 'stable' ? 'Stable' : 'Random' }}</span>
                  <span v-if="t.status === 'starting'" :class="[BADGE, 'bg-soft text-ink-3']">Starting…</span>
                </div>
                <div v-if="t.url" data-testid="tunnel-url" class="mt-0.5 truncate font-mono text-xs text-ink-2">{{ t.url }}</div>
                <div v-if="t.status !== 'error'" data-testid="countdown" class="mt-0.5 text-xs text-ink-3">{{ timeLeft(t, now) }}</div>
                <div v-if="notListening(t)" data-testid="not-listening" class="mt-1 text-xs text-warm">Port {{ t.port }} is not listening</div>
                <div v-else-if="moved(t)" data-testid="moved" class="mt-1 text-xs text-warm">{{ moved(t) }}</div>
                <div v-if="t.status === 'error'" class="mt-1 text-xs text-danger">
                  <span data-testid="tunnel-error">{{ t.error ?? 'The tunnel failed.' }}</span>
                  <button type="button" class="ml-2 underline" @click="toggleOutput(t)">{{ outputs[t.id] ? 'Hide output' : 'Show output' }}</button>
                </div>
              </div>
              <div class="flex shrink-0 items-center gap-1.5">
                <template v-if="t.url && t.status === 'open'">
                  <button type="button" :class="BTN_ROW" @click="copy(t.url, 'URL copied')">Copy</button>
                  <button type="button" :class="BTN_ROW" @click="open(t.url)">Open</button>
                </template>
                <button v-if="t.status !== 'error'" type="button" :class="BTN_ROW" @click="extend(t)">+1 h</button>
                <button type="button" :class="BTN_ROW_DANGER" @click="stop(t)">Stop</button>
              </div>
            </div>
            <pre v-if="outputs[t.id]" data-testid="tunnel-output" class="mt-2 max-h-48 overflow-auto whitespace-pre-wrap rounded-[8px] bg-soft p-2 font-mono text-[11px] text-ink-2">{{ outputs[t.id]!.join('\n') }}</pre>
          </li>
        </ul>
      </section>

      <section v-else data-testid="empty-state" class="mb-5 flex flex-col items-center gap-3 rounded-tile border border-dashed border-line-2 px-4 py-12 text-center">
        <div class="text-[13px] font-medium">No public URL yet</div>
        <p class="max-w-[360px] text-ink-3">Share a local server to get a link anyone can open until it closes.</p>
        <button type="button" data-testid="empty-share" :class="BTN_PRIMARY" @click="openShare()">Share a port</button>
      </section>

      <section v-if="stable.length > 0" aria-label="Stable URLs">
        <h2 class="mb-2 text-[11.5px] font-medium uppercase tracking-wide text-ink-3">Stable URLs</h2>
        <ul class="flex flex-col gap-2">
          <li v-for="s in stable" :key="s.cwd" data-testid="stable-row" class="flex items-center gap-3 rounded-tile border border-line bg-card px-4 py-3">
            <div class="min-w-0 flex-1">
              <div class="truncate font-mono text-xs">{{ s.hostname }}</div>
              <div class="truncate text-xs text-ink-3" :title="s.cwd">{{ folderName(s.cwd) }} · {{ s.cwd }}</div>
            </div>
            <button type="button" :class="BTN_ROW_DANGER" @click="deleteStable(s)">Delete</button>
          </li>
        </ul>
      </section>
    </div>
  </div>
</template>
