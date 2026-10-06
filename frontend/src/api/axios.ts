import axios from 'axios'

import { announceSessionCleared } from '../lib/session'

/**
 * Project submission is the slowest endpoint: the backend calls GitHub twice in
 * sequence, each with an 8s timeout, so a 15s budget could abort a request the
 * server then completed anyway — the user saw a failure for a project that had
 * in fact been created.
 */
const API_TIMEOUT_MS = 30_000
/** The refresh call must not outlive the queued 401 retries for long. */
const REFRESH_TIMEOUT_MS = 10_000

/**
 * The session lives in httpOnly cookies, so this client never holds a token:
 * `withCredentials` is what makes the browser send and accept them. It is set on
 * every request, including the refresh call, which needs the refresh cookie.
 */
export const api = axios.create({
  baseURL: '/api',
  timeout: API_TIMEOUT_MS,
  withCredentials: true,
})

/** Cookie the server sets so a double-submit check is possible. */
export const CSRF_COOKIE = 'csrf_token'
/** Header the server requires on unsafe requests that authenticate with a cookie. */
export const CSRF_HEADER = 'x-csrf-token'

/** Read one cookie by name. There is nothing secret here: it is the CSRF token. */
export function readCookie(name: string): string | null {
  const match = document.cookie
    .split(';')
    .map((part) => part.trim())
    .find((part) => part.startsWith(`${name}=`))
  return match ? decodeURIComponent(match.slice(name.length + 1)) : null
}

/**
 * Unsafe requests are the ones a cross-site page could trigger, so they have to
 * carry the CSRF header. Reads are left alone.
 */
function needsCsrf(method: string | undefined): boolean {
  const verb = (method ?? 'get').toLowerCase()
  return !['get', 'head', 'options'].includes(verb)
}

api.interceptors.request.use((config) => {
  if (needsCsrf(config.method)) {
    const token = readCookie(CSRF_COOKIE)
    if (token) {
      config.headers[CSRF_HEADER] = token
    }
  }
  return config
})

const REFRESH_PATH = '/api/auth/refresh'

/**
 * Paths that must never trigger the refresh-and-retry interceptor.
 *
 * `api` is created with `baseURL: '/api'`, so a request records `config.url` as
 * `/auth/login` — comparing against `/api/auth/login` matched nothing and a
 * failed login went through the refresh flow.
 */
const AUTH_PATHS = ['/auth/refresh', '/auth/login', '/auth/register', '/auth/logout']

export function isAuthPath(url: string | undefined): boolean {
  return url !== undefined && AUTH_PATHS.some((path) => url === path || url === `/api${path}`)
}

interface RefreshResponse {
  access_token: string
  refresh_token?: string
}

/**
 * Shared in-flight refresh so a burst of 401s triggers exactly one refresh call.
 *
 * Resolves to `true` when the session was renewed. The tokens themselves stay in
 * cookies: the browser stores whatever the server set, which is what makes this
 * work without any JavaScript-visible credential.
 */
let refreshInFlight: Promise<boolean> | null = null

export async function refreshAccessToken(): Promise<boolean> {
  try {
    // No body: the refresh cookie is the credential.
    await axios.post<RefreshResponse>(REFRESH_PATH, {}, { timeout: REFRESH_TIMEOUT_MS, withCredentials: true })
    return true
  } catch {
    return false
  }
}

/**
 * Forget the cached identity and tell the store, which would otherwise keep
 * rendering a logged-in header for a session that no longer exists.
 *
 * Only the *cache* is cleared here: the session cookies are the server's to expire,
 * and a failed refresh means they are already useless.
 */
export function clearSession(): void {
  clearProfileCache()
  announceSessionCleared()
}

/**
 * Keys that describe who is signed in, for rendering before the first API call —
 * plus the token keys an *older* build of this app wrote. A browser that upgrades
 * still holds those, and leaving a stale credential in reach of any script would
 * undo the reason for moving the session into cookies.
 */
export const PROFILE_KEYS = [
  'user_role',
  'user_name',
  'user_id',
  'user_display_name',
  'user_avatar_url',
  'access_token',
  'refresh_token',
] as const

export function clearProfileCache(): void {
  for (const key of PROFILE_KEYS) {
    localStorage.removeItem(key)
  }
}

api.interceptors.response.use(
  (response) => response,
  async (error) => {
    const original = error.config as (typeof error.config & { _retried?: boolean }) | undefined
    const status = error?.response?.status

    if (status !== 401 || !original || original._retried || isAuthPath(original.url)) {
      return Promise.reject(error)
    }

    original._retried = true
    refreshInFlight = refreshInFlight ?? refreshAccessToken()
    const renewed = await refreshInFlight
    refreshInFlight = null

    if (!renewed) {
      // The refresh cookie is gone or refused: this session is over. Whether the
      // user just signed in on another tab is not knowable here, and a false
      // "logged out" is worse than a retry the caller can make.
      clearSession()
      return Promise.reject(error)
    }
    return api.request(original)
  },
)
