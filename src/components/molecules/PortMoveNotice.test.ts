import { renderedText } from '@/test'
import { mount } from '@vue/test-utils'
import { expect, it } from 'vitest'

import PortMoveNotice from './PortMoveNotice.vue'

const notice = (): ReturnType<typeof mount> =>
  mount(PortMoveNotice, { props: { move: { from: 47_823, to: 47_824 } } })

it('names the port it left and the port it took', () => {
  expect(renderedText(notice().element)).toContain(
    'Port 47823 was taken, so AYDY moved to 47824 and updated its hooks. Sessions started before now report in again after their next message.',
  )
})

/**
 * By the time this is shown the move has been made and the hooks rewritten.
 * Drawing it as an error tells the user something is broken when it has
 * already fixed itself.
 */
it('is not an error', () => {
  expect(notice().findComponent({ name: 'Banner' }).props('variant')).toBe(
    'notice',
  )
})

/**
 * The port is not a setting. Offering one here would put back the decision the
 * app exists to make for itself.
 */
it('offers no port to choose', () => {
  const wrapper = notice()

  expect(wrapper.findAll('button')).toHaveLength(0)
  expect(wrapper.findAll('input')).toHaveLength(0)
  expect(wrapper.findAll('select')).toHaveLength(0)
})
