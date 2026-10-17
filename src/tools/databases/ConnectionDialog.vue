<script setup lang="ts">
import { computed, nextTick, ref, watch } from 'vue'
import { BTN_DIALOG, BTN_DIALOG_PRIMARY } from '../../ui/layout'
import { databasesApi } from './api'
import { useDatabases } from './store'
import type { ConnectionDraft, SslMode } from './types'

const props = defineProps<{ open: boolean; initial: ConnectionDraft | null }>()
const emit = defineEmits<{ close: []; saved: [] }>()

const { refresh } = useDatabases()

const INPUT = 'w-full rounded-[8px] border border-line-2 bg-transparent px-2.5 py-1.5 outline-none focus:border-accent'
const SSL_MODES: { value: SslMode; label: string }[] = [
  { value: 'disable', label: 'disable' },
  { value: 'prefer', label: 'prefer (certificate not verified)' },
  { value: 'require', label: 'require (certificate not verified)' },
]

const url = ref('')
const lastParsed = ref('')
const urlError = ref('')
const name = ref('')
const env = ref('')
const color = ref('#3b82f6')
const defaultColor = ref(true)
const host = ref('localhost')
const port = ref(5432)
const user = ref('postgres')
const password = ref('')
const database = ref('')
const sslMode = ref<SslMode>('prefer')
const readOnly = ref(false)
const touchedReadOnly = ref(false)
const error = ref('')
const testResult = ref<{ ok: boolean; text: string } | null>(null)
const busy = ref(false)
const urlInput = ref<HTMLInputElement | null>(null)
const nameInput = ref<HTMLInputElement | null>(null)
let parseSeq = 0

function message(e: unknown): string {
  return e instanceof Error ? e.message : String(e)
}

function reset() {
  const i = props.initial
  url.value = ''
  lastParsed.value = ''
  urlError.value = ''
  name.value = i?.name ?? ''
  env.value = i?.env ?? ''
  defaultColor.value = !i?.color
  color.value = i?.color ?? '#3b82f6'
  host.value = i?.host ?? 'localhost'
  port.value = i?.port ?? 5432
  user.value = i?.user ?? 'postgres'
  password.value = ''
  database.value = i?.database ?? ''
  sslMode.value = i?.sslMode ?? 'prefer'
  readOnly.value = i?.readOnly ?? false
  touchedReadOnly.value = false
  error.value = ''
  testResult.value = null
  busy.value = false
}
watch(
  () => [props.open, props.initial] as const,
  async ([open]) => {
    if (!open) {
      // Do not keep the pasted URL or the password in memory while closed.
      parseSeq++
      url.value = ''
      lastParsed.value = ''
      password.value = ''
      return
    }
    reset()
    await nextTick()
    ;(props.initial?.id ? nameInput.value : urlInput.value)?.focus()
  },
  { immediate: true },
)

// Read-only defaults on for staging/production; applied to user-driven env changes and to a URL parsed for a new connection only.
function applyEnvDefault() {
  if (!touchedReadOnly.value && (env.value === 'staging' || env.value === 'production')) readOnly.value = true
}

function toggleReadOnly() {
  touchedReadOnly.value = true
  readOnly.value = !readOnly.value
}

async function parse() {
  const value = url.value.trim()
  if (!value || value === lastParsed.value) return
  lastParsed.value = value
  const seq = ++parseSeq
  try {
    const p = await databasesApi.parseUrl(value)
    if (seq !== parseSeq) return
    urlError.value = ''
    if (p.name) name.value = p.name
    if (p.env !== null) {
      env.value = p.env
      if (!props.initial?.id) applyEnvDefault()
    }
    if (p.color) {
      color.value = p.color
      defaultColor.value = false
    }
    host.value = p.host
    port.value = p.port
    user.value = p.user
    if (p.password !== null) password.value = p.password
    database.value = p.database
    sslMode.value = p.sslMode
    // The pasted URL contains the password: do not keep it in the visible field.
    url.value = ''
    lastParsed.value = ''
  } catch (e) {
    if (seq === parseSeq) urlError.value = message(e)
  }
}
// Without clipboard text, the pasted value reaches the input only after the paste event.
function onPaste(event: ClipboardEvent) {
  const text = event.clipboardData?.getData('text')
  if (text) {
    url.value = text
    void parse()
  } else {
    setTimeout(() => void parse(), 0)
  }
}

const PORT_ERROR = 'Port must be between 1 and 65535.'
function portValid(): boolean {
  const n = Number(port.value)
  return String(port.value).trim() !== '' && Number.isInteger(n) && n >= 1 && n <= 65535
}

// Editing host, port or user without a password: the backend drops the saved
// one so it never reaches another server or account.
const serverChanged = computed(() => {
  const i = props.initial
  if (!i?.id) return false
  return host.value.trim() !== i.host || Number(port.value) !== i.port || user.value.trim() !== i.user
})

function draft(): ConnectionDraft {
  return {
    id: props.initial?.id ?? null,
    name: name.value.trim(),
    env: env.value.trim() || null,
    color: defaultColor.value ? null : color.value,
    host: host.value.trim(),
    port: Number(port.value),
    user: user.value.trim(),
    database: database.value.trim(),
    sslMode: sslMode.value,
    readOnly: readOnly.value,
  }
}

async function test() {
  testResult.value = null
  if (!portValid()) {
    testResult.value = { ok: false, text: PORT_ERROR }
    return
  }
  busy.value = true
  try {
    await databasesApi.test(draft(), password.value || null)
    testResult.value = { ok: true, text: 'Connected' }
  } catch (e) {
    testResult.value = { ok: false, text: message(e) }
  } finally {
    busy.value = false
  }
}

async function save() {
  error.value = ''
  if (!portValid()) {
    error.value = PORT_ERROR
    return
  }
  busy.value = true
  try {
    await databasesApi.save(draft(), password.value || null)
    await refresh()
    emit('saved')
  } catch (e) {
    error.value = message(e)
  } finally {
    busy.value = false
  }
}
</script>

<template>
  <div v-if="open" class="fixed inset-0 z-40 flex items-center justify-center bg-black/40" @keydown.esc="emit('close')">
    <form role="dialog" aria-modal="true" class="max-h-[90vh] w-[480px] overflow-auto rounded-[12px] border border-line bg-card p-5 text-[12.5px] shadow-card" @submit.prevent="save">
      <h3 class="mb-4 text-[15px] font-semibold tracking-tight">{{ initial?.id ? 'Edit connection' : 'Add connection' }}</h3>
      <div class="grid grid-cols-[110px_1fr] items-center gap-x-3 gap-y-3">
        <label for="conn-url">Paste a URL</label>
        <div>
          <input id="conn-url" ref="urlInput" v-model="url" type="text" autocomplete="off" spellcheck="false" placeholder="postgres://user:password@host:5432/database" :class="[INPUT, 'font-mono text-xs']" @paste="onPaste" @blur="parse" />
          <p v-if="urlError" data-testid="url-error" role="alert" class="mt-1 text-[11.5px] text-danger">{{ urlError }}</p>
        </div>

        <label for="conn-name">Name</label>
        <input id="conn-name" ref="nameInput" v-model="name" type="text" :class="INPUT" />

        <label for="conn-env">Env</label>
        <div>
          <input id="conn-env" v-model="env" list="conn-env-list" type="text" :class="INPUT" @input="applyEnvDefault" />
          <datalist id="conn-env-list"><option value="local" /><option value="staging" /><option value="production" /></datalist>
        </div>

        <label for="conn-color">Color</label>
        <div class="flex items-center gap-3">
          <input id="conn-color" v-model="color" type="color" :disabled="defaultColor" class="h-7 w-10 rounded-[6px] border border-line-2 bg-transparent disabled:opacity-40" />
          <label class="flex items-center gap-2"><input id="conn-color-default" v-model="defaultColor" type="checkbox" class="accent-accent" /> Default</label>
        </div>

        <label for="conn-host">Host</label>
        <input id="conn-host" v-model="host" type="text" :class="[INPUT, 'font-mono text-xs']" />

        <label for="conn-port">Port</label>
        <input id="conn-port" v-model="port" type="number" min="1" max="65535" :class="[INPUT, 'w-28 font-mono text-xs']" />

        <label for="conn-user">User</label>
        <input id="conn-user" v-model="user" type="text" autocomplete="off" :class="[INPUT, 'font-mono text-xs']" />

        <label for="conn-password">Password</label>
        <div>
          <input id="conn-password" v-model="password" type="password" autocomplete="new-password" :placeholder="initial?.id && !serverChanged ? 'Unchanged' : ''" :class="INPUT" />
          <p v-if="serverChanged && !password" data-testid="password-hint" class="mt-1 text-[11.5px] text-ink-2">Enter the password again for the new server.</p>
        </div>

        <label for="conn-database">Database</label>
        <input id="conn-database" v-model="database" type="text" :class="[INPUT, 'font-mono text-xs']" />

        <label for="conn-ssl">SSL mode</label>
        <select id="conn-ssl" v-model="sslMode" :class="INPUT">
          <option v-for="m in SSL_MODES" :key="m.value" :value="m.value">{{ m.label }}</option>
        </select>

        <span id="conn-readonly-label">Read-only</span>
        <button
          type="button"
          role="switch"
          aria-labelledby="conn-readonly-label"
          :aria-checked="readOnly ? 'true' : 'false'"
          :class="['relative h-5 w-9 rounded-full border border-line-2 transition-colors', readOnly ? 'bg-accent' : 'bg-soft']"
          @click="toggleReadOnly"
        >
          <span :class="['absolute top-[1px] h-4 w-4 rounded-full bg-card shadow transition-all', readOnly ? 'left-[17px]' : 'left-[1px]']" />
        </button>
      </div>
      <p v-if="testResult" data-testid="test-result" :role="testResult.ok ? 'status' : 'alert'" :class="['mt-3 whitespace-pre-wrap text-[12px]', testResult.ok ? 'text-ok' : 'text-danger']">{{ testResult.text }}</p>
      <p v-if="error" data-testid="form-error" role="alert" class="mt-3 rounded-[8px] border border-danger bg-soft px-3 py-2 text-danger">{{ error }}</p>
      <div class="mt-5 flex justify-end gap-2">
        <button type="button" data-action="test" :disabled="busy" :class="[BTN_DIALOG, 'mr-auto']" @click="test">Test connection</button>
        <button type="button" data-action="cancel" :class="BTN_DIALOG" @click="emit('close')">Cancel</button>
        <button type="submit" :disabled="busy" :class="BTN_DIALOG_PRIMARY">Save</button>
      </div>
    </form>
  </div>
</template>
