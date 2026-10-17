<script setup lang="ts">
import { ref, watch } from 'vue'
import { updateSettings, useSettings } from './store/settings'
import type { TerminalSetting, ThemeSetting } from '../core/types'
import { BTN_DIALOG, BTN_DIALOG_PRIMARY } from './layout'
import { refreshStatus as refreshTunnelsStatus } from '../tools/tunnels/store'

const props = defineProps<{ open: boolean }>()
const emit = defineEmits<{ close: [] }>()
const store = useSettings()

const interval = ref(3)
const terminalKind = ref<'terminal-app' | 'iterm' | 'cmux' | 'custom'>('terminal-app')
const custom = ref('')
const editor = ref('')
const notify = ref(true)
const atLogin = ref(false)
const theme = ref<ThemeSetting>('system')
const tunnelDomain = ref('')
const tunnelPrefix = ref('')
const tunnelHours = ref(4)
const error = ref<string | null>(null)

watch(
  () => props.open,
  (open) => {
    const s = store.settings
    if (!open || !s) return
    interval.value = s.pollIntervalMs / 1000
    terminalKind.value = typeof s.terminal === 'string' ? s.terminal : 'custom'
    custom.value = typeof s.terminal === 'string' ? '' : s.terminal.custom
    editor.value = s.editorCommand
    notify.value = s.notifyOnStop
    atLogin.value = s.launchAtLogin
    theme.value = s.theme
    tunnelDomain.value = s.tunnelDomain
    tunnelPrefix.value = s.tunnelPrefix
    tunnelHours.value = s.tunnelHours
    error.value = null
  },
  { immediate: true },
)

async function save() {
  error.value = null
  if (terminalKind.value === 'custom' && !custom.value.includes('{cmd}')) {
    error.value = 'The custom terminal command must contain {cmd}.'
    return
  }
  const terminal: TerminalSetting = terminalKind.value === 'custom' ? { custom: custom.value } : terminalKind.value
  try {
    await updateSettings({
      pollIntervalMs: Math.round(Number(interval.value) * 1000),
      terminal,
      editorCommand: editor.value,
      notifyOnStop: notify.value,
      launchAtLogin: atLogin.value,
      theme: theme.value,
      tunnelDomain: tunnelDomain.value,
      tunnelPrefix: tunnelPrefix.value,
      tunnelHours: Number(tunnelHours.value),
    })
    // The tunnel domain shown by Tunnels comes from tunnels_status.
    void refreshTunnelsStatus()
    emit('close')
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e)
  }
}
</script>

<template>
  <div v-if="open" class="fixed inset-0 z-40 flex items-center justify-center bg-black/40">
    <form role="dialog" aria-modal="true" class="w-[480px] rounded-[12px] border border-line bg-card p-5 text-[12.5px] shadow-card" @submit.prevent="save">
      <h3 class="mb-4 text-[15px] font-semibold tracking-tight">Settings</h3>
      <div class="grid grid-cols-[150px_1fr] items-center gap-x-3 gap-y-3.5">
        <label for="interval">Refresh interval</label>
        <div>
          <input id="interval" v-model="interval" name="interval" type="number" min="2" max="10" step="1" class="rounded-[8px] border border-line-2 bg-transparent px-2.5 py-1.5 outline-none focus:border-accent" />
          <p class="mt-1 text-[11.5px] text-ink-3">Seconds, from 2 to 10.</p>
        </div>

        <label for="terminal">Terminal</label>
        <select id="terminal" v-model="terminalKind" name="terminal" class="rounded-[8px] border border-line-2 bg-transparent px-2.5 py-1.5 outline-none focus:border-accent">
          <option value="terminal-app">Terminal.app</option>
          <option value="iterm">iTerm2</option>
          <option value="cmux">cmux</option>
          <option value="custom">Custom command</option>
        </select>

        <template v-if="terminalKind === 'cmux'">
          <span />
          <p data-testid="cmux-hint" class="-mt-2 text-[11.5px] text-ink-3">
            cmux only accepts commands from its own terminals by default. In cmux, open Settings › Automation and change the socket control mode.
          </p>
        </template>

        <template v-if="terminalKind === 'custom'">
          <label for="custom">Command</label>
          <div>
            <input id="custom" v-model="custom" name="custom" type="text" class="w-full rounded-[8px] border border-line-2 bg-transparent px-2.5 py-1.5 outline-none focus:border-accent font-mono text-xs" />
            <p class="mt-1 text-[11.5px] text-ink-3">Use {cmd} where the command goes. It runs in your login shell.</p>
          </div>
        </template>

        <label for="editor">Editor command</label>
        <div>
          <input id="editor" v-model="editor" name="editor" type="text" class="w-full rounded-[8px] border border-line-2 bg-transparent px-2.5 py-1.5 outline-none focus:border-accent font-mono text-xs" />
          <p class="mt-1 text-[11.5px] text-ink-3">Called with the project directory, e.g. <span class="font-mono">code</span>, <span class="font-mono">cursor</span>, <span class="font-mono">zed</span>.</p>
        </div>

        <label for="theme">Theme</label>
        <select id="theme" v-model="theme" name="theme" class="rounded-[8px] border border-line-2 bg-transparent px-2.5 py-1.5 outline-none focus:border-accent">
          <option value="system">System</option>
          <option value="light">Light</option>
          <option value="dark">Dark</option>
        </select>

        <span>Tunnels</span>
        <div class="flex flex-col gap-3">
          <div>
            <label for="tunnelDomain" class="mb-1 block text-ink-2">Domain</label>
            <input id="tunnelDomain" v-model="tunnelDomain" name="tunnelDomain" type="text" placeholder="dev.example.com" class="w-full rounded-[8px] border border-line-2 bg-transparent px-2.5 py-1.5 font-mono text-xs outline-none focus:border-accent" />
          </div>
          <div>
            <label for="tunnelPrefix" class="mb-1 block text-ink-2">Prefix</label>
            <input id="tunnelPrefix" v-model="tunnelPrefix" name="tunnelPrefix" type="text" placeholder="your macOS user name" class="w-full rounded-[8px] border border-line-2 bg-transparent px-2.5 py-1.5 font-mono text-xs outline-none focus:border-accent" />
          </div>
          <div>
            <label for="tunnelHours" class="mb-1 block text-ink-2">Default duration</label>
            <select id="tunnelHours" v-model="tunnelHours" name="tunnelHours" class="rounded-[8px] border border-line-2 bg-transparent px-2.5 py-1.5 outline-none focus:border-accent">
              <option :value="1">1 h</option>
              <option :value="4">4 h</option>
              <option :value="8">8 h</option>
              <option :value="24">24 h</option>
            </select>
          </div>
        </div>

        <span>General</span>
        <div class="flex flex-col gap-2">
          <label class="flex items-center gap-2"><input v-model="notify" type="checkbox" name="notify" class="accent-accent" /> Notify when a service stops</label>
          <label class="flex items-center gap-2"><input v-model="atLogin" type="checkbox" name="login" class="accent-accent" /> Launch at login</label>
        </div>
      </div>
      <p v-if="error" role="alert" class="mt-3 rounded-[8px] border border-danger bg-soft px-3 py-2 text-danger">{{ error }}</p>
      <div class="mt-5 flex justify-end gap-2">
        <button type="button" :class="BTN_DIALOG" @click="emit('close')">Cancel</button>
        <button type="submit" :class="BTN_DIALOG_PRIMARY">Save</button>
      </div>
    </form>
  </div>
</template>
