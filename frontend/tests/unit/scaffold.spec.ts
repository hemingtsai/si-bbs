import { mount } from '@vue/test-utils'
import { describe, expect, it } from 'vitest'
import Home from '../../src/views/Home.vue'

describe('app scaffold', () => {
  it('renders the home view', () => {
    const wrapper = mount(Home)
    expect(wrapper.text()).toContain('SI BBS')
  })
})