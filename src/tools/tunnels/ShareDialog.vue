<script setup lang="ts">
import { computed, nextTick, ref, watch } from 'vue'
import { BTN_DIALOG, BTN_DIALOG_PRIMARY } from '../../ui/layout'
import { useSettings } from '../../ui/store/settings'
import { toasts } from '../../ui/store/toasts'
import { useServices } from '../services/store/services'
import { tunnelsApi } from './api'
import { BAD_DOMAIN, closeShare, refreshStatus, useTunnels, validDomain } from './store'
import type { OpenRequest, Tunnel } from './types'

const { state } = useTunnels()
const settings = useSettings()
const { state: services } = useServices()

const INPUT = 'w-full rounded-[8px] border border-line-2 bg-transparent px-2.5 py-1.5 outline-none focus:border-accent'
const NAME_RULE = /^[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?$/
const NAME_ERROR = 'Use lowercase letters, digits and dashes (63 characters at most).'
const HOURS = [1, 4, 8, 24] as const

type Target = { port: number; project: string; cwd: string | null }

const port = ref<number | null>(null)
const project = ref('')
const cwd = ref<string | null>(null)
const kind = ref<'quick' | 'stable'>('quick')
const name = ref('')
const remembered = ref(false)
const hours = ref<OpenRequest['hours']>(4)
const nameError = ref('')
const busy = ref(false)
const portSelect = ref<HTMLSelectElement | null>(null)
let nameSeq = 0

/** Listening ports of the Services snapshot: processes and running containers, one entry per port. */
const listening = computed<Target[]>(() => {
  const snap = services.snapshot
  const found = new Map<number, Target>()
  if (snap) {
    for (const p of snap.processes) for (const x of p.ports) if (!found.has(x.port)) found.set(x.port, { port: x.port, project: p.project, cwd: p.cwd })
    const containers = [...snap.standalone, ...snap.compose.flatMap((g) => g.containers)]
    for (const c of containers) {
      if (c.state !== 'running') continue
      for (const x of c.ports) if (!found.has(x.port)) found.set(x.port, { port: x.port, project: c.name, cwd: null })
    }
  }
  return [...found.values()].sort((a, b) => a.port - b.port)
})
const options = computed<Target[]>(() => {
  const list = listening.value
  if (port.value !== null && !list.some((t) => t.port === port.value)) {
    return [{ port: port.value, project: project.value, cwd: cwd.value }, ...list]
  }
  return list
})

const tunnelStatus = computed(() => state.status)
const stableReason = computed<string | null>(() => {
  const s = tunnelStatus.value
  if (!s) return 'Checking cloudflared…'
  if (!s.binary) return 'cloudflared is not installed. Run: brew install cloudflared'
  if (!s.loggedIn) return 'Connect cloudflared to a Cloudflare account first.'
  if (!s.domain) return 'Set a tunnel domain in Settings first.'
  if (!validDomain(s.domain)) return BAD_DOMAIN
  return null
})
const domain = computed(() => tunnelStatus.value?.domain ?? '')
/** Full hostname remembered for this folder: it may use another domain than the current setting. */
const rememberedHost = computed<string | null>(() => {
  if (!remembered.value || !cwd.value) return null
  return tunnelStatus.value?.stable.find((e) => e.cwd === cwd.value)?.hostname ?? null
})

function resetFrom(target: Target | null) {
  port.value = target?.port ?? null
  project.value = target?.project ?? ''
  cwd.value = target?.cwd ?? null
}

function clearName() {
  nameSeq++
  name.value = ''
  remembered.value = false
}

async function loadName() {
  clearName()
  const seq = nameSeq
  try {
    const s = await tunnelsApi.suggestName(cwd.value, project.value)
    if (seq !== nameSeq) return
    name.value = s.subdomain
    remembered.value = s.remembered
  } catch (e) {
    if (seq === nameSeq) toasts.error(e instanceof Error ? e.message : String(e))
  }
}

watch(
  () => state.share.open,
  async (open) => {
    if (!open) return
    const share = state.share
    resetFrom(share.port !== null ? { port: share.port, project: share.project, cwd: share.cwd } : (listening.value[0] ?? null))
    kind.value = 'quick'
    clearName()
    nameError.value = ''
    busy.value = false
    const h = settings.settings?.tunnelHours
    hours.value = (HOURS as readonly number[]).includes(h ?? 0) ? (h as OpenRequest['hours']) : 4
    void refreshStatus()
    await nextTick()
    portSelect.value?.focus()
  },
  { immediate: true },
)

function onPort(event: Event) {
  const n = Number((event.target as HTMLSelectElement).value)
  resetFrom(options.value.find((t) => t.port === n) ?? null)
  clearName()
  if (kind.value === 'stable') void loadName()
}

function pickKind(value: 'quick' | 'stable') {
  if (value === 'stable' && stableReason.value) return
  kind.value = value
  nameError.value = ''
  if (value === 'stable') void loadName()
}

watch(name, () => (nameError.value = ''))

// Opened tunnels whose URL must be copied once they report `open`.
const pending = new Set<string>()
const seen = new Set<string>()

async function settle(t: Tunnel) {
  if (t.status === 'open' && t.url) {
    pending.delete(t.id)
    try {
      await navigator.clipboard.writeText(t.url)
      toasts.success('Tunnel open — URL copied')
    } catch (e) {
      toasts.error(e instanceof Error ? e.message : String(e))
    }
  } else if (t.status === 'error') {
    pending.delete(t.id)
    toasts.error(t.error ?? 'The tunnel failed to open.')
  }
}

watch(
  () => state.tunnels,
  (list) => {
    for (const id of [...pending]) {
      const t = list.find((x) => x.id === id)
      if (t) {
        seen.add(id)
        void settle(t)
      } else if (seen.has(id)) {
        pending.delete(id)
        seen.delete(id)
      }
    }
  },
  { deep: true },
)

async function share() {
  if (busy.value || port.value === null) return
  let subdomain: string | null = null
  if (kind.value === 'stable') {
    subdomain = name.value.trim()
    if (!remembered.value && !NAME_RULE.test(subdomain)) {
      nameError.value = NAME_ERROR
      return
    }
  }
  busy.value = true
  try {
    const t = await tunnelsApi.open({ port: port.value, project: project.value, cwd: cwd.value, kind: kind.value, subdomain, hours: hours.value })
    // A new stable URL is now remembered in tunnels.json.
    if (t.kind === 'stable') void refreshStatus()
    closeShare()
    toasts.success('Opening tunnel…')
    pending.add(t.id)
    // An `open` event may have landed before this response: settle with the store's entry.
    void settle(state.tunnels.find((x) => x.id === t.id) ?? t)
  } catch (e) {
    toasts.error(e instanceof Error ? e.message : String(e))
  } finally {
    busy.value = false
  }
}
</script>

<template>
  <div v-if="state.share.open" class="fixed inset-0 z-40 flex items-center justify-center bg-black/40" @keydown.esc="closeShare()">
    <form role="dialog" aria-modal="true" aria-labelledby="share-title" class="max-h-[90vh] w-[480px] overflow-auto rounded-[12px] border border-line bg-card p-5 text-[12.5px] shadow-card" @submit.prevent="share">
      <h3 id="share-title" class="mb-4 text-[15px] font-semibold tracking-tight">Share publicly</h3>
      <div class="grid grid-cols-[110px_1fr] items-start gap-x-3 gap-y-3">
        <label for="share-port" class="pt-1.5">Port</label>
        <select id="share-port" ref="portSelect" :value="port ?? ''" :class="[INPUT, 'font-mono text-xs']" @change="onPort">
          <option v-if="options.length === 0" value="" disabled>No listening port</option>
          <option v-for="o in options" :key="o.port" :value="o.port">:{{ o.port }}{{ o.project ? ` — ${o.project}` : '' }}</option>
        </select>

        <span class="pt-0.5">Type</span>
        <div>
          <label class="mr-4 inline-flex items-center gap-2"><input v-model="kind" type="radio" name="share-kind" value="quick" class="accent-accent" @change="pickKind('quick')" /> Random URL</label>
          <label :class="['inline-flex items-center gap-2', stableReason ? 'text-ink-3' : '']"><input :checked="kind === 'stable'" type="radio" name="share-kind" value="stable" :disabled="stableReason !== null" class="accent-accent" @change="pickKind('stable')" /> Stable URL</label>
          <p v-if="stableReason" data-testid="stable-reason" class="mt-1 text-[11.5px] text-ink-3">{{ stableReason }}</p>
        </div>

        <template v-if="kind === 'stable'">
          <label :for="rememberedHost ? undefined : 'share-name'" class="pt-1.5">Name</label>
          <div>
            <div v-if="rememberedHost" data-testid="share-hostname" class="pt-1.5 font-mono text-xs">{{ rememberedHost }}</div>
            <div v-else class="flex items-center gap-1.5">
              <input id="share-name" v-model="name" type="text" autocomplete="off" spellcheck="false" :readonly="remembered" :class="[INPUT, 'font-mono text-xs', remembered ? 'opacity-70' : '']" />
              <span data-testid="share-suffix" class="whitespace-nowrap font-mono text-xs text-ink-2">.{{ domain }}</span>
            </div>
            <p v-if="nameError" data-testid="name-error" role="alert" class="mt-1 text-[11.5px] text-danger">{{ nameError }}</p>
          </div>
        </template>

        <label for="share-hours" class="pt-1.5">Duration</label>
        <select id="share-hours" v-model.number="hours" :class="[INPUT, 'w-28']">
          <option v-for="h in HOURS" :key="h" :value="h">{{ h }} h</option>
        </select>
      </div>
      <p data-testid="share-warning" class="mt-4 text-[11.5px] text-ink-2">Anyone with the link can reach this server until it closes.</p>
      <div class="mt-4 flex justify-end gap-2">
        <button type="button" :class="BTN_DIALOG" @click="closeShare()">Cancel</button>
        <button type="submit" :class="BTN_DIALOG_PRIMARY" :disabled="busy || port === null">Share</button>
      </div>
    </form>
  </div>
</template>
