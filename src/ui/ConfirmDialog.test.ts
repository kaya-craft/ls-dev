import { describe, expect, it } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import ConfirmDialog from './ConfirmDialog.vue'
import { answer, confirm } from './store/confirm'

describe('ConfirmDialog', () => {
  it('focuses the confirm button when a request opens', async () => {
    const wrapper = mount(ConfirmDialog, { attachTo: document.body })
    const pending = confirm({ title: 't', body: 'b', confirmLabel: 'Go' })
    await flushPromises()
    const buttons = wrapper.findAll('button')
    expect(document.activeElement).toBe(buttons[buttons.length - 1].element)
    answer({ ok: false, checked: false })
    await pending
    wrapper.unmount()
  })
})
