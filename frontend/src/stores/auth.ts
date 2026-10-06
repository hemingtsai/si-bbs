import { defineStore } from 'pinia'
import { computed, ref } from 'vue'

import { clearSession } from '../api/axios'
import { authApi } from '../api'
import { SESSION_CLEARED_EVENT } from '../lib/session'
import type { AuthTokens, Me, ProfileInput, Role } from '../api/types'

const STORAGE = {
  access: 'access_token',
  refresh: 'refresh_token',
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
  const accessToken = ref<string | null>(localStorage.getItem(STORAGE.access))
  // What to show in the UI. Falls back to the login name when no display name is set.
  const displayName = ref<string | null>(localStorage.getItem(STORAGE.displayName))
  const avatarUrl = ref<string | null>(localStorage.getItem(STORAGE.avatar))

  const isAuthenticated = computed(() => accessToken.value !== null)
  const shownName = computed(() => displayName.value ?? username.value)
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
    const { data } = await authApi.login(payload)
    persist(data)
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
      display_name: data.display_name,
      avatar_url: data.avatar_url,
    })
    return data
  }

  /// Changing the password invalidates every token, including this session's, so the
  /// fresh pair from the server has to be stored or the next request would 401.
  async function changePassword(payload: {
    current_password: string
    new_password: string
  }): Promise<void> {
    const { data } = await authApi.changePassword(payload)
    persist(data)
  }

  function reset(): void {
    displayName.value = null
    avatarUrl.value = null
    localStorage.removeItem(STORAGE.displayName)
    localStorage.removeItem(STORAGE.avatar)
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
    updateProfile,
    changePassword,
    logout,
    persist,
    shownName,
    displayName,
    avatarUrl,
  }
})