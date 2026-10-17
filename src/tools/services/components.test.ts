import { describe, expect, it } from 'vitest'
import { mount } from '@vue/test-utils'
import ServiceIcon from './ServiceIcon.vue'
import Meter from './Meter.vue'
import { ctr, proc } from '../../test/fixtures'

describe('ServiceIcon', () => {
  it('renders the label and tone classes', () => {
    const w = mount(ServiceIcon, { props: { service: ctr({ image: 'redis:7' }) } })
    expect(w.text()).toBe('Rd')
    expect(w.classes()).toContain('bg-p-rd-bg')
    expect(w.classes()).toContain('text-p-rd')
    expect(w.attributes('aria-hidden')).toBe('true')
  })
  it('uses the node tone for a process', () => {
    const w = mount(ServiceIcon, { props: { service: proc() } })
    expect(w.text()).toBe('N')
    expect(w.classes()).toContain('bg-p-node-bg')
  })
})

describe('Meter', () => {
  it('sets the fill width and aria-valuenow', () => {
    const w = mount(Meter, { props: { fraction: 0.5, kind: 'cpu' } })
    expect(w.attributes('role')).toBe('meter')
    expect(w.attributes('aria-valuenow')).toBe('50')
    const fill = w.get('[data-fill]')
    expect((fill.element as HTMLElement).style.width).toBe('50%')
    expect(fill.classes()).toContain('bg-accent')
  })
  it('uses the warm colour for mem', () => {
    const w = mount(Meter, { props: { fraction: 0.2, kind: 'mem' } })
    expect(w.get('[data-fill]').classes()).toContain('bg-warm')
  })
  const width = (f: number) => {
    const w = mount(Meter, { props: { fraction: f, kind: 'cpu' } })
    return [(w.get('[data-fill]').element as HTMLElement).style.width, w.attributes('aria-valuenow')]
  }
  it('clamps above 1', () => expect(width(1.5)).toEqual(['100%', '100']))
  it('clamps below 0', () => {
    expect(width(-1)).toEqual(['0%', '0'])
    expect(width(0)).toEqual(['0%', '0'])
  })
  it('treats non-finite as 0', () => {
    expect(width(NaN)).toEqual(['0%', '0'])
    expect(width(Infinity)).toEqual(['0%', '0'])
  })
  it('has an accessible name from kind', () => {
    expect(mount(Meter, { props: { fraction: 0.1, kind: 'cpu' } }).attributes('aria-label')).toBe('CPU')
    expect(mount(Meter, { props: { fraction: 0.1, kind: 'mem' } }).attributes('aria-label')).toBe('Memory')
  })
})
