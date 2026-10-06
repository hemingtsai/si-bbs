import { defineStore } from 'pinia'
import { computed, ref } from 'vue'

import { clearProfileCache, clearSession } from '../api/axios'
import { authApi } from '../api'
import { SESSION_CLEARED_EVENT } from '../lib/session'
import type { AuthTokens, Me, ProfileInput, Role } from '../api/types'

/**
 * Only the *identity* is cached, for rendering the header before the first API call
 * answers. The session itself lives in httpOnly cookies the browser sends for us,
 * so nothing here is a credential — an XSS bug can read a name, not a session.
 */
const STORAGE = {
  role: 'user_role',
  name: 'user_name',
  displayName: 'user_display_name',
  avatar: 'user_avatar_url',
} as const

export const useAuthStore = defineStore('auth', () => {
  const role = ref<Role | null>((localStorage.getItem(STORAGE.role) as Role | null) ?? null)
  const username = ref<string | null>(localStorage.getItem(STORAGE.name))
  const userId = ref<number | null>(
    localStorage.getItem('user_id') !== null ? Number(localStorage.getItem('user_id')) : null,
  )
  /// Set once a session is confirmed by the server (see `ensureSession`).
  const known = ref(false)
  // What to show in the UI. Falls back to the login name when no display name is set.
  const displayName = ref<string | null>(localStorage.getItem(STORAGE.displayName))
  const avatarUrl = ref<string | null>(localStorage.getItem(STORAGE.avatar))

  const isAuthenticated = computed(() => known.value)
  const shownName = computed(() => displayName.value ?? username.value)
  const isStaff = computed(() => role.value === 'admin' || role.value === 'moderator')
  const isAdmin = computed(() => role.value === 'admin')

  function persist(tokens: Partial<AuthTokens>): void {
    if (tokens.role) {
      role.value = tokens.role
      localStorage.setItem(STORAGE.role, tokens.role)
    }
    if (tokens.username) {
      username.value = tokens.username
      localStorage.setItem(STORAGE.name, tokens.username)
    }
    if (tokens.user_id) {
      userId.value = tokens.user_id
      localStorage.setItem('user_id', String(tokens.user_id))
    }
    if ('display_name' in tokens) {
      displayName.value = tokens.display_name ?? null
      setOrRemove(STORAGE.displayName, tokens.display_name)
    }
    if ('avatar_url' in tokens) {
      avatarUrl.value = tokens.avatar_url ?? null
      setOrRemove(STORAGE.avatar, tokens.avatar_url)
    }
  }

  function setOrRemove(key: string, value: string | null | undefined): void {
    if (value) {
      localStorage.setItem(key, value)
    } else {
      localStorage.removeItem(key)
    }
  }

  async function login(payload: { username: string; password: string }): Promise<void> {
    // The server answers with the tokens *and* sets the cookies; the browser client
    // only needs the identity, which `fetchMe` then reads back.
    await authApi.login(payload)
    known.value = true
    await fetchMe()
    // The boot check ran before this login and, if the visitor was a stranger,
    // memorised "no session". Replacing it is what stops the router from bouncing
    // the freshly signed-in user back to the login page on the next navigation.
    bootCheck = Promise.resolve(true)
  }

  /**
   * Establish whether a session exists, once per page load.
   *
   * There is no synchronous way to know: the cookies are httpOnly, so the answer
   * comes from the server. The router guard awaits this, which is why a signed-in
   * user no longer flashes the login page on a full reload.
   */
  let bootCheck: Promise<boolean> | null = null
  async function ensureSession(): Promise<boolean> {
    // Memoised on purpose: the guard runs on every navigation, and the answer cannot
    // change without a login, a logout or a session the server has dropped.
    bootCheck ??= (async () => {
      try {
        await fetchMe()
        known.value = true
        return true
      } catch {
        known.value = false
        clearProfileCache()
        // `forget()` and not `reset()`: emptying `bootCheck` here would re-ask the
        // server on every navigation a signed-out visitor makes.
        forget()
        return false
      }
    })()
    return bootCheck
  }

  async function register(payload: {
    username: string
    email: string
    password: string
  }): Promise<void> {
    await authApi.register(payload)
  }

  async function fetchMe(): Promise<Me> {
    const { data } = await authApi.profile()
    // Trust the server, not the cached copy in localStorage.
    persist({
      role: data.role,
      username: data.username,
      // `user_id` is what every "is this mine?" check compares; dropping it hid the
      // delete and edit controls on a user's own content.
      user_id: data.id,
      display_name: data.display_name,
      avatar_url: data.avatar_url,
    })
    return data
  }

  /// Save profile fields and mirror the result into the cached header data.
  async function updateProfile(payload: ProfileInput): Promise<Me> {
    const { data } = await authApi.updateProfile(payload)
    persist({
      role: data.role,
      username: data.username,
      user_id: data.id,
      display_name: data.display_name,
      avatar_url: data.avatar_url,
    })
    return data
  }

  /// Changing the password invalidates every token, so the server reissues the
  /// cookies in the same response. Nothing to store here — the browser does it.
  async function changePassword(payload: {
    current_password: string
    new_password: string
  }): Promise<void> {
    await authApi.changePassword(payload)
  }

  /// Tell the server to drop the cookies, then forget the cached identity. A failure
  /// still clears locally: the user asked to be signed out.
  async function logout(): Promise<void> {
    try {
      await authApi.logout()
    } catch {
      // Ignored on purpose: signing out locally must always succeed.
    }
    clearSession()
    reset()
  }

  /// Drop every trace of the previous session, without touching the memoised check.
  function forget(): void {
    displayName.value = null
    avatarUrl.value = null
    role.value = null
    username.value = null
    userId.value = null
    known.value = false
    for (const key of Object.values(STORAGE)) {
      localStorage.removeItem(key)
    }
  }

  /// Forget the session *and* let the next navigation ask the server again.
  function reset(): void {
    forget()
    bootCheck = null
  }

  // The HTTP layer can drop the session behind the store's back: a refresh that
  // fails while the app is running calls `clearSession()`, which empties
  // localStorage but knows nothing about these refs. Without this the sidebar
  // kept showing a user whose every request then 401s.
  if (typeof window !== 'undefined') {
    window.addEventListener(SESSION_CLEARED_EVENT, reset)
  }

  function forgetLocally(): void {
    clearSession()
    reset()
  }

  return {
    role,
    username,
    userId,
    known,
    isAuthenticated,
    isStaff,
    isAdmin,
    login,
    register,
    fetchMe,
    ensureSession,
    updateProfile,
    changePassword,
    logout,
    forgetLocally,
    persist,
    shownName,
    displayName,
    avatarUrl,
  }
})