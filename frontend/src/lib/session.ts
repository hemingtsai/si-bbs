/**
 * Session lifecycle signal shared by the HTTP layer and the store.
 *
 * `clearSession()` lives in `api/axios.ts` because the response interceptor is
 * what discovers that a refresh token is dead, but the Pinia store owns the
 * reactive state the UI renders from. Importing the store from the HTTP layer
 * would be a cycle, so the two are connected by a plain DOM event instead.
 */
export const SESSION_CLEARED_EVENT = 'si-bbs:session-cleared'

export function announceSessionCleared(): void {
  if (typeof window === 'undefined') return
  window.dispatchEvent(new Event(SESSION_CLEARED_EVENT))
}
