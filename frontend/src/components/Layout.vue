<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { RouterLink, RouterView, useRoute, useRouter } from 'vue-router'

import { useAuthStore } from '../stores/auth'

const auth = useAuthStore()
const router = useRouter()
const route = useRoute()

type ThemePref = 'system' | 'light' | 'dark'
const themePref = ref<ThemePref>('system')

function resolveTheme(pref: ThemePref): 'light' | 'dark' {
  if (pref !== 'system') return pref
  return window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light'
}

function applyTheme(): void {
  document.documentElement.dataset.theme = resolveTheme(themePref.value)
  localStorage.setItem('si-bbs-theme', themePref.value)
}

function cycleTheme(): void {
  themePref.value = themePref.value === 'system' ? 'light' : themePref.value === 'light' ? 'dark' : 'system'
  applyTheme()
}

onMounted(() => {
  const stored = localStorage.getItem('si-bbs-theme')
  if (stored === 'light' || stored === 'dark') themePref.value = stored
  applyTheme()
  const media = window.matchMedia('(prefers-color-scheme: dark)')
  media.addEventListener('change', applyTheme)
})

function logout(): void {
  auth.logout()
  router.push({ name: 'home' })
}

const searchTerm = ref('')

/// Search is a page, not a dropdown: results are linkable and reloadable.
function goSearch(): void {
  const q = searchTerm.value.trim()
  if (!q) return
  router.push({ name: 'search', query: { q } })
}

const themeLabel = () => (themePref.value === 'system' ? '跟随系统' : themePref.value === 'light' ? '浅色' : '深色')
</script>

<template>
  <div class="shell">
    <aside class="sidebar">
      <RouterLink to="/" class="sidebar-brand">SI BBS<span class="mono">v0.1</span></RouterLink>
      <RouterLink to="/wiki" class="nav-item">Wiki</RouterLink>
      <RouterLink to="/projects" class="nav-item">项目</RouterLink>
      <RouterLink to="/forum" class="nav-item">论坛</RouterLink>
      <RouterLink v-if="auth.isStaff" to="/moderation" class="nav-item">审核</RouterLink>
      <RouterLink v-if="auth.isAdmin" to="/admin" class="nav-item">管理</RouterLink>
      <RouterLink v-if="auth.isAuthenticated" to="/me" class="nav-item">我的</RouterLink>
      <RouterLink v-if="auth.isAuthenticated" to="/settings" class="nav-item">设置</RouterLink>
      <RouterLink v-if="auth.isAuthenticated" to="/trash" class="nav-item">回收站</RouterLink>
      <RouterLink v-if="!auth.isAuthenticated" to="/login" class="nav-item">登录</RouterLink>
      <form class="sidebar-search" @submit.prevent="goSearch">
        <input v-model="searchTerm" type="search" placeholder="搜索…" aria-label="搜索" />
      </form>
      <div class="sidebar-spacer"></div>
      <div class="sidebar-foot">
        <RouterLink v-if="auth.isAuthenticated" to="/settings" class="user user-link">
          <img v-if="auth.avatarUrl" class="user-avatar" :src="auth.avatarUrl" alt="" />
          {{ auth.shownName ?? '未登录' }}
        </RouterLink>
        <span v-else class="user">未登录</span>
        <a class="linklike" href="/feed.xml">RSS</a>
        <button class="linklike" @click="cycleTheme">{{ themeLabel() }}</button>
        <button v-if="auth.isAuthenticated" class="linklike" @click="logout">退出</button>
      </div>
    </aside>
    <div class="main">
      <div class="mainbar">
        <span class="page-title">{{ String(route.name ?? '') }}</span>
        <div class="mainbar-right">
          <form class="mainbar-search" @submit.prevent="goSearch">
            <input v-model="searchTerm" type="search" placeholder="搜索…" aria-label="搜索站点" />
          </form>
          <!-- Only visible on <= 768px via .mobile-nav-only -->
          <div class="mobile-nav-only">
            <RouterLink v-if="auth.isAuthenticated" to="/settings" class="user user-link">
              {{ auth.shownName }}
            </RouterLink>
            <span v-else class="user">未登录</span>
            <a class="linklike" href="/feed.xml">RSS</a>
        <button class="linklike" @click="cycleTheme">{{ themeLabel() }}</button>
            <button v-if="auth.isAuthenticated" class="linklike" @click="logout">退出</button>
          </div>
        </div>
      </div>
      <main class="content">
        <RouterView />
      </main>
    </div>
  </div>
</template>
