import { afterEach, describe, expect, it, vi } from 'vitest'

import { api, applyRefreshResponse, clearSession, isAuthPath } from '../../src/api/axios'
import { SESSION_CLEARED_EVENT } from '../../src/lib/session'

describe('clearSession', () => {
  afterEach(() => localStorage.clear())

  it('removes every persisted key and announces the change', () => {
    localStorage.setItem('access_token', 'a')
    localStorage.setItem('refresh_token', 'r')
    localStorage.setItem('user_role', 'admin')
    localStorage.setItem('user_name', 'alice')
    localStorage.setItem('user_id', '1')

    const listener = vi.fn()
    window.addEventListener(SESSION_CLEARED_EVENT, listener)
    try {
      clearSession()
    } finally {
      window.removeEventListener(SESSION_CLEARED_EVENT, listener)
    }

    expect(localStorage.length).toBe(0)
    // The store cannot see localStorage; the event is how it learns.
    expect(listener).toHaveBeenCalledTimes(1)
  })
})

describe('applyRefreshResponse', () => {
  afterEach(() => localStorage.clear())

  it('keeps the rotated refresh token instead of discarding it', () => {
    localStorage.setItem('access_token', 'old-access')
    localStorage.setItem('refresh_token', 'old-refresh')

    const access = applyRefreshResponse({ access_token: 'new-access', refresh_token: 'new-refresh' })

    expect(access).toBe('new-access')
    expect(localStorage.getItem('access_token')).toBe('new-access')
    expect(localStorage.getItem('refresh_token')).toBe('new-refresh')
  })

  it('leaves the stored refresh token alone when the server omits it', () => {
    localStorage.setItem('refresh_token', 'old-refresh')
    applyRefreshResponse({ access_token: 'new-access' })
    expect(localStorage.getItem('refresh_token')).toBe('old-refresh')
  })
})

describe('isAuthPath', () => {
  it('recognises the path as axios actually records it', () => {
    // The instance has baseURL '/api', so the request config holds '/auth/login'.
    // The interceptor used to compare against '/api/auth/login' and therefore
    // never matched, sending failed logins through the refresh flow.
    expect(isAuthPath('/auth/login')).toBe(true)
    expect(isAuthPath('/auth/register')).toBe(true)
    expect(isAuthPath('/auth/refresh')).toBe(true)
  })

  it('recognises the absolute spelling too', () => {
    expect(isAuthPath('/api/auth/login')).toBe(true)
  })

  it('ignores everything else', () => {
    expect(isAuthPath('/projects')).toBe(false)
    expect(isAuthPath('/auth/me')).toBe(false)
    expect(isAuthPath(undefined)).toBe(false)
  })

  it('records request paths relative to the baseURL', async () => {
    const original = api.defaults.adapter
    let seen: string | undefined
    api.defaults.adapter = (async (config) => {
      seen = config.url
      return { data: null, status: 200, statusText: 'OK', headers: {}, config }
    }) as typeof api.defaults.adapter

    try {
      await api.get('/auth/login')
    } finally {
      api.defaults.adapter = original
    }

    expect(seen).toBe('/auth/login')
    expect(isAuthPath(seen)).toBe(true)
  })
})
