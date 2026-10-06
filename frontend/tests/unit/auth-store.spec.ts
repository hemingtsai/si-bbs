import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { api, clearSession } from '../../src/api/axios'
import { announceSessionCleared } from '../../src/lib/session'
import { useAuthStore } from '../../src/stores/auth'

// The store is unit-tested against a stubbed API module, so no HTTP layer or
// axios interceptors are involved here.
vi.mock('../../src/api/axios', () => ({
  api: { post: vi.fn(), get: vi.fn() },
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

  it('persists tokens and role on login', async () => {
    vi.mocked(api.post).mockResolvedValue({
      data: {
        access_token: 'a1',
        refresh_token: 'r1',
        role: 'moderator',
        user_id: 7,
        username: 'alice',
      },
    })

    const store = useAuthStore()
    await store.login({ username: 'alice', password: 'pw' })

    expect(store.isAuthenticated).toBe(true)
    expect(store.isStaff).toBe(true)
    expect(store.isAdmin).toBe(false)
    expect(store.username).toBe('alice')
    expect(localStorage.getItem('access_token')).toBe('a1')
    expect(localStorage.getItem('refresh_token')).toBe('r1')
    expect(localStorage.getItem('user_role')).toBe('moderator')
  })

  it('treats admin as staff', () => {
    localStorage.setItem('access_token', 'a1')
    localStorage.setItem('user_role', 'admin')
    const store = useAuthStore()
    expect(store.isAdmin).toBe(true)
    expect(store.isStaff).toBe(true)
  })

  it('clears everything on logout', async () => {
    localStorage.setItem('access_token', 'a1')
    localStorage.setItem('refresh_token', 'r1')
    localStorage.setItem('user_role', 'admin')
    localStorage.setItem('user_name', 'alice')
    setActivePinia(createPinia())

    const store = useAuthStore()
    store.logout()

    expect(store.isAuthenticated).toBe(false)
    expect(localStorage.getItem('access_token')).toBeNull()
    expect(localStorage.getItem('refresh_token')).toBeNull()
    expect(localStorage.getItem('user_role')).toBeNull()
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

  it('stores the fresh token pair a password change returns', async () => {
    localStorage.setItem('access_token', 'old-access')
    const { useAuthStore } = await import('../../src/stores/auth')
    api.post = vi.fn().mockResolvedValue({
      data: {
        access_token: 'new-access',
        refresh_token: 'new-refresh',
        role: 'user',
        user_id: 4,
        username: 'dave',
      },
    })
    const store = useAuthStore()
    await store.changePassword({ current_password: 'a', new_password: 'b' })
    // Without this the next request would 401: the server killed the old token.
    expect(store.accessToken).toBe('new-access')
    expect(localStorage.getItem('access_token')).toBe('new-access')
    expect(localStorage.getItem('refresh_token')).toBe('new-refresh')
  })
})
