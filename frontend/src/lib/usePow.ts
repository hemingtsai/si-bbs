import { onUnmounted, ref, type Ref } from 'vue'

import { onSolvingChange } from './pow'

/**
 * Whether a challenge is currently being solved.
 *
 * The work is deliberately invisible when it is fast, but a submit button that says
 * "提交中…" while the browser is hashing is telling the user something untrue. Views
 * use this to say "验证中…" instead.
 */
export function useSolving(): Ref<boolean> {
  const solving = ref(false)
  const off = onSolvingChange((value) => {
    solving.value = value
  })
  onUnmounted(off)
  return solving
}
