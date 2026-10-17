<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { basicSetup } from 'codemirror'
import { Annotation, EditorState, Prec, StateEffect, StateField, Transaction } from '@codemirror/state'
import { Decoration, EditorView, keymap, type DecorationSet } from '@codemirror/view'
import { HighlightStyle, syntaxHighlighting } from '@codemirror/language'
import { tags } from '@lezer/highlight'
import { PostgreSQL, sql } from '@codemirror/lang-sql'

const props = withDefaults(defineProps<{ modelValue: string; history?: string[] }>(), { history: () => [] })
const emit = defineEmits<{ 'update:modelValue': [value: string]; run: [] }>()

const host = ref<HTMLElement | null>(null)
let view: EditorView | null = null

/** Marks a document replacement made by this component, not typed by the user. */
const programmatic = Annotation.define<boolean>()

// Error line: one line decoration, mapped through edits, replaced by `setErrorLine`.
const setErrorLine = StateEffect.define<number | null>()
const errorLine = StateField.define<DecorationSet>({
  create: () => Decoration.none,
  update(deco, tr) {
    let next = deco.map(tr.changes)
    for (const e of tr.effects) {
      if (!e.is(setErrorLine)) continue
      next = e.value === null ? Decoration.none : Decoration.set([Decoration.line({ class: 'cm-error-line' }).range(e.value)])
    }
    return next
  },
  provide: (f) => EditorView.decorations.from(f),
})

// History stepping: -1 is the text being written, stashed when the first step leaves it.
let historyIndex = -1
let stash = ''

function replaceDoc(text: string) {
  if (!view) return
  view.dispatch({
    changes: { from: 0, to: view.state.doc.length, insert: text },
    selection: { anchor: text.length },
    annotations: [programmatic.of(true), Transaction.addToHistory.of(false)],
  })
}

function stepHistory(direction: 1 | -1): boolean {
  const list = props.history
  if (!view || list.length === 0) return true
  const next = Math.max(-1, Math.min(list.length - 1, historyIndex + direction))
  if (next === historyIndex) return true
  if (historyIndex === -1) stash = view.state.doc.toString()
  historyIndex = next
  const text = next === -1 ? stash : list[next]
  replaceDoc(text)
  emit('update:modelValue', text)
  return true
}

const theme = EditorView.theme({
  '&': { height: '100%', backgroundColor: 'var(--color-card)', color: 'var(--color-ink)', fontSize: '12.5px' },
  '&.cm-focused': { outline: 'none' },
  '.cm-scroller': { fontFamily: 'var(--font-mono)', lineHeight: '20px' },
  '.cm-content': { caretColor: 'var(--color-ink)' },
  '.cm-cursor, .cm-dropCursor': { borderLeftColor: 'var(--color-ink)' },
  '.cm-gutters': { backgroundColor: 'var(--color-card)', color: 'var(--color-idle)', borderRight: '1px solid var(--color-line)' },
  '.cm-activeLine': { backgroundColor: 'var(--color-row-hover)' },
  '.cm-activeLineGutter': { backgroundColor: 'var(--color-row-hover)', color: 'var(--color-ink-2)' },
  '&.cm-focused > .cm-scroller > .cm-selectionLayer .cm-selectionBackground, .cm-selectionBackground, .cm-content ::selection': {
    backgroundColor: 'var(--color-accent-soft)',
  },
  '.cm-selectionMatch': { backgroundColor: 'var(--color-accent-soft)' },
  '&.cm-focused .cm-matchingBracket': { backgroundColor: 'var(--color-hover)', outline: '1px solid var(--color-line-2)' },
  '.cm-foldPlaceholder': { backgroundColor: 'var(--color-soft)', border: '1px solid var(--color-line-2)', color: 'var(--color-ink-2)' },
  '.cm-tooltip': { backgroundColor: 'var(--color-card)', border: '1px solid var(--color-line-2)', color: 'var(--color-ink)' },
  '.cm-tooltip-autocomplete > ul > li[aria-selected]': { backgroundColor: 'var(--color-accent-soft)', color: 'var(--color-ink)' },
  '.cm-panels': { backgroundColor: 'var(--color-soft)', color: 'var(--color-ink)' },
  '.cm-panels.cm-panels-bottom': { borderTop: '1px solid var(--color-line)' },
  '.cm-textfield': { backgroundColor: 'var(--color-card)', border: '1px solid var(--color-line-2)', color: 'var(--color-ink)' },
  '.cm-button': { backgroundImage: 'none', backgroundColor: 'var(--color-card)', border: '1px solid var(--color-line-2)', color: 'var(--color-ink)' },
  '.cm-searchMatch': { backgroundColor: 'var(--color-accent-soft)' },
  '.cm-error-line': { textDecoration: 'underline wavy var(--color-danger)', textDecorationSkipInk: 'none', textUnderlineOffset: '3px' },
})

// Token colours from tokens.css so the editor follows the light and dark themes.
const highlight = HighlightStyle.define([
  { tag: [tags.keyword, tags.operatorKeyword, tags.modifier], color: 'var(--color-accent)' },
  { tag: [tags.string, tags.special(tags.string)], color: 'var(--color-ok)' },
  { tag: [tags.number, tags.bool, tags.null], color: 'var(--color-warm)' },
  { tag: [tags.lineComment, tags.blockComment, tags.comment], color: 'var(--color-ink-3)', fontStyle: 'italic' },
  { tag: [tags.typeName, tags.standard(tags.name)], color: 'var(--color-p-pg)' },
  { tag: [tags.operator, tags.punctuation, tags.bracket], color: 'var(--color-ink-2)' },
  { tag: tags.invalid, color: 'var(--color-danger)' },
])

onMounted(() => {
  view = new EditorView({
    parent: host.value!,
    state: EditorState.create({
      doc: props.modelValue,
      extensions: [
        // Above basicSetup: its default keymap binds Mod-Enter and Mod-ArrowUp/Down.
        Prec.highest(
          keymap.of([
            { key: 'Mod-Enter', run: () => (emit('run'), true) },
            { key: 'Mod-ArrowUp', run: () => stepHistory(1) },
            { key: 'Mod-ArrowDown', run: () => stepHistory(-1) },
          ]),
        ),
        basicSetup,
        sql({ dialect: PostgreSQL }),
        syntaxHighlighting(highlight),
        theme,
        errorLine,
        EditorView.updateListener.of((u) => {
          if (!u.docChanged || u.transactions.some((t) => t.annotation(programmatic))) return
          historyIndex = -1
          emit('update:modelValue', u.state.doc.toString())
        }),
      ],
    }),
  })
})

onBeforeUnmount(() => {
  view?.destroy()
  view = null
})

// External changes (another connection's draft) replace the doc only when different, to keep the cursor.
watch(
  () => props.modelValue,
  (value) => {
    if (view && value !== view.state.doc.toString()) {
      historyIndex = -1
      replaceDoc(value)
    }
  },
)
watch(
  () => props.history,
  () => {
    historyIndex = -1
  },
)

/** Underlines the line holding Postgres' 1-based `position`; `null` clears it. */
function markErrorLine(position: number | null) {
  if (!view) return
  if (position === null) {
    view.dispatch({ effects: setErrorLine.of(null) })
    return
  }
  const doc = view.state.doc
  const offset = Math.max(0, Math.min(doc.length, position - 1))
  view.dispatch({ effects: setErrorLine.of(doc.lineAt(offset).from) })
}

function focus() {
  view?.focus()
}

defineExpose({ markErrorLine, focus })
</script>

<template>
  <div ref="host" class="h-full min-h-0" />
</template>
