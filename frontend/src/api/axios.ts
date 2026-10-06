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

export const api = axios.create({
  baseURL: '/api',
  timeout: API_TIMEOUT_MS,
})

api.interceptors.request.use((config) => {
  const token = localStorage.getItem('access_token')
  if (token) {
    config.headers.Authorization = `Bearer ${token}`
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
const AUTH_PATHS = ['/auth/refresh', '/auth/login', '/auth/register']

export function isAuthPath(url: string | undefined): boolean {
  return url !== undefined && AUTH_PATHS.some((path) => url === path || url === `/api${path}`)
}

interface RefreshResponse {
  access_token: string
  refresh_token?: string
}

/**
 * Store the tokens from a refresh response.
 *
 * The endpoint returns a **rotated pair**. Keeping only the access token capped
 * every session at seven days from the original login, and would break every
 * session outright the moment the server starts invalidating used refresh tokens.
 */
export function applyRefreshResponse(data: RefreshResponse): string {
  localStorage.setItem('access_token', data.access_token)
  if (data.refresh_token) {
    localStorage.setItem('refresh_token', data.refresh_token)
  }
  return data.access_token
}

/**
 * Shared in-flight refresh so a burst of 401s triggers exactly one refresh call.
 */
let refreshInFlight: Promise<string | null> | null = null

export async function refreshAccessToken(refresh: string): Promise<string | null> {
  try {
    const { data } = await axios.post<RefreshResponse>(
      REFRESH_PATH,
      { refresh_token: refresh },
      { timeout: REFRESH_TIMEOUT_MS },
    )
    return applyRefreshResponse(data)
  } catch {
    // Drop the session only if this is still the session we tried to refresh.
    //
    // A password change invalidates every token, and the store installs the fresh
    // pair the server returned. A request that was already in flight with the *old*
    // token then fails, and its refresh fails too — without this check that stale
    // failure would wipe the brand-new pair and log the user out of the session
    // they just created.
    if (localStorage.getItem('refresh_token') === refresh) {
      clearSession()
    }
    return null
  }
}

/**
 * Remove every persisted credential and tell the store, which would otherwise
 * keep rendering a logged-in header for a session that no longer exists.
 */
export function clearSession(): void {
  localStorage.removeItem('access_token')
  localStorage.removeItem('refresh_token')
  localStorage.removeItem('user_role')
  localStorage.removeItem('user_name')
  localStorage.removeItem('user_id')
  announceSessionCleared()
}

api.interceptors.response.use(
  (response) => response,
  async (error) => {
    const original = error.config as (typeof error.config & { _retried?: boolean }) | undefined
    const status = error?.response?.status

    if (status !== 401 || !original || original._retried || isAuthPath(original.url)) {
      return Promise.reject(error)
    }
    const staleRefresh = localStorage.getItem('refresh_token')
    if (!staleRefresh) {
      return Promise.reject(error)
    }

    original._retried = true
    refreshInFlight = refreshInFlight ?? refreshAccessToken(staleRefresh)
    const token = await refreshInFlight
    refreshInFlight = null

    if (!token) {
      return Promise.reject(error)
    }
    original.headers.Authorization = `Bearer ${token}`
    return api.request(original)
  },
)
