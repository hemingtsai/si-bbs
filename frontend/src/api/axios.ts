import axios from 'axios'

export const api = axios.create({
  baseURL: '/api',
  timeout: 15000,
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
 * Shared in-flight refresh so that a burst of 401s triggers exactly one
 * refresh call. Without this, ten parallel requests would each burn the refresh
 * token and invalidate each other's rotated pair.
 */
let refreshInFlight: Promise<string | null> | null = null

async function refreshAccessToken(): Promise<string | null> {
  const refresh = localStorage.getItem('refresh_token')
  if (!refresh) return null

  try {
    const { data } = await axios.post<{ access_token: string }>(REFRESH_PATH, {
      refresh_token: refresh,
    })
    localStorage.setItem('access_token', data.access_token)
    return data.access_token
  } catch {
    // The refresh token expired or was revoked: drop the session.
    clearSession()
    return null
  }
}

export function clearSession(): void {
  localStorage.removeItem('access_token')
  localStorage.removeItem('refresh_token')
  localStorage.removeItem('user_role')
  localStorage.removeItem('user_name')
}

api.interceptors.response.use(
  (response) => response,
  async (error) => {
    const original = error.config as (typeof error.config & { _retried?: boolean }) | undefined
    const status = error?.response?.status

    const isAuthRoute =
      original?.url === REFRESH_PATH ||
      original?.url === '/api/auth/login' ||
      original?.url === '/api/auth/register'

    if (status !== 401 || !original || original._retried || isAuthRoute) {
      return Promise.reject(error)
    }
    if (!localStorage.getItem('refresh_token')) {
      return Promise.reject(error)
    }

    original._retried = true
    refreshInFlight = refreshInFlight ?? refreshAccessToken()
    const token = await refreshInFlight
    refreshInFlight = null

    if (!token) {
      return Promise.reject(error)
    }
    original.headers.Authorization = `Bearer ${token}`
    return api.request(original)
  },
)