// Columns: ports, service (flexible), uptime, cpu, memory. Row actions overlay the row on hover.
export const ROW_GRID =
  'grid grid-cols-[128px_minmax(0,1fr)_64px_120px_120px] items-center gap-3 px-4'

// Below 960 px: no uptime column.
export const ROW_GRID_NARROW =
  'grid grid-cols-[128px_minmax(0,1fr)_120px_120px] items-center gap-3 px-4'

// Compact table (table card narrower than 640 px): no uptime column, no meters.
export const ROW_GRID_COMPACT =
  'grid grid-cols-[96px_minmax(0,1fr)_64px_80px] items-center gap-3 px-4'

export const BTN =
  'rounded-[8px] border border-line-2 px-2 py-0.5 text-xs hover:bg-hover disabled:opacity-40'

export const BTN_DANGER =
  'rounded-[8px] border border-danger px-2 py-0.5 text-xs text-danger hover:bg-hover disabled:opacity-40'

// 26 px buttons for the table rows and Compose headers only.
export const BTN_ROW =
  'inline-flex h-[26px] items-center rounded-[8px] border border-line-2 px-2.5 text-xs hover:bg-hover disabled:opacity-40'

export const BTN_ROW_DANGER =
  'inline-flex h-[26px] items-center rounded-[8px] border border-danger px-2.5 text-xs text-danger hover:bg-hover disabled:opacity-40'

export const BTN_GHOST =
  'inline-flex h-6 items-center whitespace-nowrap rounded-[8px] px-2.5 text-xs font-normal text-ink-2 hover:bg-hover disabled:opacity-40'

export const BTN_PAGE =
  'inline-flex h-8 items-center gap-1.5 whitespace-nowrap rounded-nav border border-line-2 bg-card px-3.5 text-[12.5px] hover:bg-hover disabled:opacity-40'

export const BTN_PRIMARY =
  'inline-flex h-8 items-center gap-1.5 whitespace-nowrap rounded-nav border border-accent bg-accent px-3.5 text-[12.5px] font-medium text-accent-fg hover:opacity-90 disabled:opacity-40'

// Dialog buttons: 32 px high, 8 px radius (BTN_PAGE / BTN_PRIMARY use the 9 px nav radius).
export const BTN_DIALOG =
  'inline-flex h-8 items-center whitespace-nowrap rounded-[8px] border border-line-2 bg-card px-3.5 text-[12.5px] hover:bg-hover disabled:opacity-40'

export const BTN_DIALOG_PRIMARY =
  'inline-flex h-8 items-center whitespace-nowrap rounded-[8px] border border-accent bg-accent px-3.5 text-[12.5px] font-medium text-accent-fg hover:opacity-90 disabled:opacity-40'

export const BTN_DIALOG_DANGER =
  'inline-flex h-8 items-center whitespace-nowrap rounded-[8px] border border-danger bg-danger px-3.5 text-[12.5px] font-medium text-accent-fg hover:opacity-90 disabled:opacity-40'
