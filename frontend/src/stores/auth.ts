import { defineStore } from 'pinia'
import { computed, ref } from 'vue'

import { api, clearSession } from '../api/axios'
import { SESSION_CLEARED_EVENT } from '../lib/session'
import type { AuthTokens, Me, Role } from '../api/types'

const STORAGE = {
  access: 'access_token',
  refresh: 'refresh_token',
  role: 'user_role',
  name: 'user_name',
} as const

export const useAuthStore = defineStore('auth', () => {
  const role = ref<Role | null>((localStorage.getItem(STORAGE.role) as Role | null) ?? null)
  const username = ref<string | null>(localStorage.getItem(STORAGE.name))
  const userId = ref<number | null>(
    localStorage.getItem('user_id') !== null ? Number(localStorage.getItem('user_id')) : null,
  )
  const accessToken = ref<string | null>(localStorage.getItem(STORAGE.access))

  const isAuthenticated = computed(() => accessToken.value !== null)
  const isStaff = computed(() => role.value === 'admin' || role.value === 'moderator')
  const isAdmin = computed(() => role.value === 'admin')

  function persist(tokens: Partial<AuthTokens>): void {
    if (tokens.access_token) {
      accessToken.value = tokens.access_token
      localStorage.setItem(STORAGE.access, tokens.access_token)
    }
    if (tokens.refresh_token) {
      localStorage.setItem(STORAGE.refresh, tokens.refresh_token)
    }
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
  }

  async function login(payload: { username: string; password: string }): Promise<void> {
    const { data } = await api.post<AuthTokens>('/auth/login', payload)
    persist(data)
  }

  async function register(payload: {
    username: string
    email: string
    password: string
  }): Promise<void> {
    await api.post('/auth/register', payload)
  }

  async function fetchMe(): Promise<Me> {
    const { data } = await api.get<Me>('/auth/me')
    // Trust the server, not the cached copy in localStorage.
    persist({ role: data.role, username: data.username })
    return data
  }

  function reset(): void {
    role.value = null
    username.value = null
    userId.value = null
    accessToken.value = null
  }

  // The HTTP layer can drop the session behind the store's back: a refresh that
  // fails while the app is running calls `clearSession()`, which empties
  // localStorage but knows nothing about these refs. Without this the sidebar
  // kept showing a user whose every request then 401s.
  if (typeof window !== 'undefined') {
    window.addEventListener(SESSION_CLEARED_EVENT, reset)
  }

  function logout(): void {
    clearSession()
    reset()
  }

  return {
    role,
    username,
    userId,
    accessToken,
    isAuthenticated,
    isStaff,
    isAdmin,
    login,
    register,
    fetchMe,
    logout,
    persist,
  }
})