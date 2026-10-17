import { enableAutoUnmount, mount } from '@vue/test-utils'
import { afterEach, describe, expect, it } from 'vitest'
import SqlEditor from './SqlEditor.vue'

enableAutoUnmount(afterEach)

// jsdom has no layout: these tests only cover the document, decorations and key bindings.
const isMac = /Mac/.test(navigator.platform)
function press(w: ReturnType<typeof mount>, key: string) {
  const content = w.find('.cm-content').element
  content.dispatchEvent(new KeyboardEvent('keydown', { key, metaKey: isMac, ctrlKey: !isMac, bubbles: true, cancelable: true }))
}

describe('SqlEditor', () => {
  it('shows the model value and follows external changes', async () => {
    const w = mount(SqlEditor, { props: { modelValue: 'select 1' }, attachTo: document.body })
    expect(w.find('.cm-content').text()).toContain('select 1')
    await w.setProps({ modelValue: 'select 2' })
    expect(w.find('.cm-content').text()).toContain('select 2')
    expect(w.emitted('update:modelValue')).toBeUndefined()
  })

  it('Mod-Enter emits run', () => {
    const w = mount(SqlEditor, { props: { modelValue: 'select 1' }, attachTo: document.body })
    press(w, 'Enter')
    expect(w.emitted('run')).toHaveLength(1)
  })

  it('Mod-ArrowUp and Mod-ArrowDown step through the history and back to the draft', async () => {
    const w = mount(SqlEditor, { props: { modelValue: 'draft', history: ['q1', 'q2'] }, attachTo: document.body })
    press(w, 'ArrowUp')
    press(w, 'ArrowUp')
    press(w, 'ArrowUp')
    press(w, 'ArrowDown')
    press(w, 'ArrowDown')
    expect(w.emitted('update:modelValue')).toEqual([['q1'], ['q2'], ['q1'], ['draft']])
  })

  it('markErrorLine underlines the line of the position and null clears it', async () => {
    const w = mount(SqlEditor, { props: { modelValue: 'select\nfrom x' }, attachTo: document.body })
    const vm = w.vm as unknown as { markErrorLine: (p: number | null) => void }
    vm.markErrorLine(8)
    await w.vm.$nextTick()
    const marked = w.findAll('.cm-line.cm-error-line')
    expect(marked).toHaveLength(1)
    expect(marked[0].text()).toBe('from x')
    vm.markErrorLine(null)
    await w.vm.$nextTick()
    expect(w.findAll('.cm-error-line')).toHaveLength(0)
  })
})
