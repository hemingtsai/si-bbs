import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { api, clearSession } from '../../src/api/axios'
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
      data: { id: 1, username: 'alice', role: 'user', banned: false },
    })

    const store = useAuthStore()
    const me = await store.fetchMe()

    expect(me.role).toBe('user')
    expect(store.role).toBe('user')
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
})