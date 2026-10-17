import { describe, expect, it } from 'vitest'
import { isNarrow } from './viewport'

describe('isNarrow', () => {
  it('follows the window width on resize', () => {
    window.innerWidth = 800
    window.dispatchEvent(new Event('resize'))
    expect(isNarrow.value).toBe(true)
    window.innerWidth = 1200
    window.dispatchEvent(new Event('resize'))
    expect(isNarrow.value).toBe(false)
  })
})

describe('railShown', () => {
  it('is true when narrow or when the sidebar is collapsed', async () => {
    const { railShown } = await import('./viewport')
    const { toggleSidebar } = await import('./navigation')
    isNarrow.value = false
    expect(railShown.value).toBe(false)
    isNarrow.value = true
    expect(railShown.value).toBe(true)
    isNarrow.value = false
    toggleSidebar()
    expect(railShown.value).toBe(true)
    toggleSidebar()
    expect(railShown.value).toBe(false)
  })
})
