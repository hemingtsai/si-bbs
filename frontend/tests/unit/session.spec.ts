import { afterEach, describe, expect, it, vi } from 'vitest'

import axios from 'axios'

import {
  api,
  clearProfileCache,
  clearSession,
  isAuthPath,
  readCookie,
  refreshAccessToken,
} from '../../src/api/axios'
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

describe('clearProfileCache', () => {
  afterEach(() => localStorage.clear())

  it('drops the cached identity but leaves unrelated keys alone', () => {
    localStorage.setItem('user_role', 'admin')
    localStorage.setItem('user_name', 'alice')
    localStorage.setItem('user_id', '1')
    localStorage.setItem('user_display_name', '爱丽丝')
    localStorage.setItem('user_avatar_url', 'https://cdn.example/a.png')
    localStorage.setItem('theme', 'dark')

    clearProfileCache()

    // Nothing here is a credential any more — the session is in cookies — but a
    // stale name in the header is its own kind of wrong.
    for (const key of ['user_role', 'user_name', 'user_id', 'user_display_name', 'user_avatar_url']) {
      expect(localStorage.getItem(key)).toBeNull()
    }
    expect(localStorage.getItem('theme')).toBe('dark')
  })
})

describe('readCookie', () => {
  afterEach(() => {
    document.cookie = 'csrf_token=; Max-Age=0; Path=/'
  })

  it('reads the CSRF cookie and nothing else', () => {
    document.cookie = 'csrf_token=abc123; Path=/'
    expect(readCookie('csrf_token')).toBe('abc123')
    expect(readCookie('access_token')).toBeNull()
  })

  it('does not match a cookie whose name merely ends with the same text', () => {
    document.cookie = 'not_csrf_token=nope; Path=/'
    expect(readCookie('csrf_token')).toBeNull()
  })
})

describe('refreshAccessToken', () => {
  afterEach(() => {
    localStorage.clear()
    vi.restoreAllMocks()
  })

  it('reports success without storing anything: the cookie did the work', async () => {
    // The server rotates the cookies in the response; JavaScript never sees a token.
    const post = vi
      .spyOn(axios, 'post')
      .mockResolvedValue({ data: { access_token: 'new', refresh_token: 'new-r' } })

    await expect(refreshAccessToken()).resolves.toBe(true)

    expect(post).toHaveBeenCalledTimes(1)
    expect(localStorage.length).toBe(0)
  })

  it('reports failure so the caller can drop the session', async () => {
    vi.spyOn(axios, 'post').mockRejectedValue(new Error('401'))
    await expect(refreshAccessToken()).resolves.toBe(false)
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

