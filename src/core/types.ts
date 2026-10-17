export type TerminalSetting = 'terminal-app' | 'iterm' | 'cmux' | { custom: string }

export type ThemeSetting = 'system' | 'light' | 'dark'

export type Settings = {
  pollIntervalMs: number
  showAll: boolean
  terminal: TerminalSetting
  editorCommand: string
  notifyOnStop: boolean
  launchAtLogin: boolean
  theme: ThemeSetting
  worktreeRoots: string[]
  worktreeExcludes: string[]
  tunnelDomain: string
  tunnelPrefix: string
  tunnelHours: number
}
