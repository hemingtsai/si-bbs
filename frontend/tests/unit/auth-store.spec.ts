import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { api, clearSession } from '../../src/api/axios'
import { announceSessionCleared } from '../../src/lib/session'
import { useAuthStore } from '../../src/stores/auth'

// The store is unit-tested against a stubbed API module, so no HTTP layer or
// axios interceptors are involved here.
vi.mock('../../src/api/axios', () => ({
  api: { post: vi.fn(), get: vi.fn() },
  clearProfileCache: vi.fn(() => {
    for (const key of ['user_role', 'user_name', 'user_id', 'user_display_name', 'user_avatar_url']) {
      localStorage.removeItem(key)
    }
  }),
  clearSession: vi.fn(() => {
    localStorage.removeItem('access_token')
    localStorage.removeItem('refresh_token')
    localStorage.removeItem('user_role')
    localStorage.removeItem('user_name')
    localStorage.removeItem('user_id')
  }),
}))

describe('auth store', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    localStorage.clear()
    vi.clearAllMocks()
    setActivePinia(createPinia())
  })

  it('starts logged out', () => {
    const store = useAuthStore()
    expect(store.isAuthenticated).toBe(false)
    expect(store.isStaff).toBe(false)
    expect(store.isAdmin).toBe(false)
  })

  it('marks the session as known on login and stores no token', async () => {
    vi.mocked(api.post).mockResolvedValue({
      data: { access_token: 'a1', refresh_token: 'r1', role: 'moderator', user_id: 7, username: 'alice' },
    })
    vi.mocked(api.get).mockResolvedValue({
      data: {
        id: 7,
        username: 'alice',
        display_name: null,
        email: 'alice@example.com',
        bio: null,
        avatar_url: null,
        role: 'moderator',
        banned: false,
        created_at: '2025-01-01 00:00:00',
      },
    })

    const store = useAuthStore()
    await store.login({ username: 'alice', password: 'pw' })

    expect(store.isAuthenticated).toBe(true)
    expect(store.isStaff).toBe(true)
    expect(store.isAdmin).toBe(false)
    expect(store.username).toBe('alice')
    expect(store.role).toBe('moderator')
    // The whole point of the cookie session: no credential in JavaScript-reachable
    // storage. Only the identity cache is written.
    expect(localStorage.getItem('access_token')).toBeNull()
    expect(localStorage.getItem('refresh_token')).toBeNull()
    expect(localStorage.getItem('user_role')).toBe('moderator')
  })

  it('treats admin as staff from the cached role', () => {
    localStorage.setItem('user_role', 'admin')
    const store = useAuthStore()
    expect(store.isAdmin).toBe(true)
    expect(store.isStaff).toBe(true)
    // The role is a rendering hint only; `isAuthenticated` waits for the server.
    expect(store.isAuthenticated).toBe(false)
  })

  it('clears the cached identity on logout and tells the server', async () => {
    localStorage.setItem('user_role', 'admin')
    localStorage.setItem('user_name', 'alice')
    localStorage.setItem('user_display_name', '爱丽丝')
    setActivePinia(createPinia())
    const post = vi.mocked(api.post).mockResolvedValue({ data: { status: 'ok' } })

    const store = useAuthStore()
    await store.logout()

    expect(store.isAuthenticated).toBe(false)
    expect(store.username).toBeNull()
    expect(localStorage.getItem('user_role')).toBeNull()
    expect(localStorage.getItem('user_name')).toBeNull()
    expect(localStorage.getItem('user_display_name')).toBeNull()
    // The cookies are the server's to clear, so the request has to happen.
    expect(post).toHaveBeenCalledWith('/auth/logout')
  })

  it('still signs out locally when the logout request fails', async () => {
    localStorage.setItem('user_role', 'admin')
    setActivePinia(createPinia())
    vi.mocked(api.post).mockRejectedValue(new Error('offline'))

    const store = useAuthStore()
    await store.logout()

    expect(store.isAuthenticated).toBe(false)
    expect(localStorage.getItem('user_role')).toBeNull()
  })

  it('asks the server once per page load whether a session exists', async () => {
    setActivePinia(createPinia())
    const get = vi.mocked(api.get).mockRejectedValue({ response: { status: 401 } })

    const store = useAuthStore()
    // Two navigations must not mean two round trips.
    await expect(store.ensureSession()).resolves.toBe(false)
    await expect(store.ensureSession()).resolves.toBe(false)
    expect(get).toHaveBeenCalledTimes(1)
    expect(store.isAuthenticated).toBe(false)
  })

  it('overwrites a stale cached role with the server value', async () => {
    localStorage.setItem('access_token', 'a1')
    localStorage.setItem('user_role', 'admin')
    setActivePinia(createPinia())

    vi.mocked(api.get).mockResolvedValue({
      data: {
        id: 1,
        username: 'alice',
        display_name: '爱丽丝',
        email: 'alice@example.com',
        bio: null,
        avatar_url: 'https://cdn.example/a.png',
        role: 'user',
        banned: false,
        created_at: '2025-01-01 00:00:00',
      },
    })

    const store = useAuthStore()
    const me = await store.fetchMe()

    expect(me.role).toBe('user')
    expect(store.role).toBe('user')
    // Ownership checks compare ids ("is this comment mine?"), so the id has to be
    // cached alongside the name. Forgetting it hid every delete/edit control.
    expect(store.userId).toBe(1)
    expect(localStorage.getItem('user_id')).toBe('1')
    // The account payload also carries the profile fields, and the shell needs them.
    expect(store.displayName).toBe('爱丽丝')
    expect(store.shownName).toBe('爱丽丝')
    expect(store.avatarUrl).toBe('https://cdn.example/a.png')
    expect(store.isAdmin).toBe(false)
  })

  it('clearSession removes every persisted key', () => {
    localStorage.setItem('access_token', 'a')
    localStorage.setItem('refresh_token', 'r')
    localStorage.setItem('user_role', 'user')
    localStorage.setItem('user_name', 'bob')
    localStorage.setItem('user_id', '3')
    clearSession()
    expect(localStorage.length).toBe(0)
  })

  it('drops the in-memory session when the HTTP layer clears it', () => {
    localStorage.setItem('access_token', 'a1')
    localStorage.setItem('user_role', 'admin')
    localStorage.setItem('user_name', 'alice')
    setActivePinia(createPinia())

    const store = useAuthStore()
    expect(store.isAdmin).toBe(true)

    // What `clearSession()` does when a refresh fails mid-session: the store
    // itself is never told, except through this event.
    announceSessionCleared()

    expect(store.isAuthenticated).toBe(false)
    expect(store.isAdmin).toBe(false)
    expect(store.role).toBeNull()
    expect(store.username).toBeNull()
    expect(store.userId).toBeNull()
  })
})
describe('auth store profile handling', () => {
  beforeEach(() => {
    localStorage.clear()
    vi.clearAllMocks()
    setActivePinia(createPinia())
  })

  it('falls back to the login name when there is no display name', async () => {
    const { useAuthStore } = await import('../../src/stores/auth')
    api.get = vi.fn().mockResolvedValue({
      data: {
        id: 2,
        username: 'bob',
        display_name: null,
        email: 'bob@example.com',
        bio: null,
        avatar_url: null,
        role: 'user',
        banned: false,
        created_at: '2025-01-01 00:00:00',
      },
    })
    const store = useAuthStore()
    await store.fetchMe()
    expect(store.shownName).toBe('bob')
    expect(localStorage.getItem('user_display_name')).toBeNull()
  })

  it('clears the cached display name when the server says there is none', async () => {
    localStorage.setItem('user_display_name', '旧昵称')
    localStorage.setItem('user_avatar_url', 'https://cdn.example/old.png')
    const { useAuthStore } = await import('../../src/stores/auth')
    api.patch = vi.fn().mockResolvedValue({
      data: {
        id: 3,
        username: 'carol',
        display_name: null,
        email: 'carol@example.com',
        bio: null,
        avatar_url: null,
        role: 'user',
        banned: false,
        created_at: '2025-01-01 00:00:00',
      },
    })
    const store = useAuthStore()
    await store.updateProfile({ display_name: '' })
    expect(store.displayName).toBeNull()
    expect(localStorage.getItem('user_display_name')).toBeNull()
    expect(localStorage.getItem('user_avatar_url')).toBeNull()
  })

  it('keeps the session usable after a password change, with no token in JS', async () => {
    const { useAuthStore } = await import('../../src/stores/auth')
    api.post = vi.fn().mockResolvedValue({
      data: { access_token: 'new-access', refresh_token: 'new-refresh', role: 'user', user_id: 4, username: 'dave' },
    })
    const store = useAuthStore()
    await store.changePassword({ current_password: 'a', new_password: 'b' })
    // The server reissued the cookies in that same response; the client stores
    // nothing, which is what makes the swap impossible to get wrong.
    expect(localStorage.getItem('access_token')).toBeNull()
    expect(localStorage.getItem('refresh_token')).toBeNull()
  })
})
