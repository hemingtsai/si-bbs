<script setup lang="ts">
import { useRouter } from 'vue-router'

import { useAuthStore } from '../stores/auth'

const auth = useAuthStore()
const router = useRouter()

function logout(): void {
  auth.logout()
  router.push({ name: 'home' })
}
</script>

<template>
  <div class="layout">
    <header class="layout__header">
      <RouterLink to="/" class="layout__brand">SI BBS</RouterLink>
      <nav class="layout__nav">
        <RouterLink to="/wiki">Wiki</RouterLink>
        <RouterLink to="/projects">项目</RouterLink>
        <RouterLink v-if="auth.isStaff" to="/moderation">审核</RouterLink>
        <RouterLink v-if="auth.isAuthenticated" to="/me">我的</RouterLink>
        <RouterLink v-if="auth.isAuthenticated" to="/trash">回收站</RouterLink>
        <RouterLink v-if="auth.isAdmin" to="/admin">管理</RouterLink>
        <template v-if="auth.isAuthenticated">
          <span class="layout__user">{{ auth.username }}</span>
          <button class="layout__action" @click="logout">退出</button>
        </template>
        <RouterLink v-else to="/login">登录</RouterLink>
      </nav>
    </header>
    <main class="layout__main">
      <RouterView />
    </main>
  </div>
</template>

<style scoped>
.layout__header {
  display: flex;
  align-items: center;
  gap: 1.5rem;
  padding: 0.75rem 1.25rem;
  border-bottom: 1px solid var(--si-border);
}
.layout__brand {
  font-weight: 700;
  text-decoration: none;
  color: inherit;
}
.layout__nav {
  display: flex;
  gap: 1rem;
  align-items: center;
}
.layout__user {
  color: var(--si-muted, #666);
  font-size: 0.9rem;
}
.layout__action {
  border: none;
  background: none;
  cursor: pointer;
  color: inherit;
  font: inherit;
  padding: 0;
}
.layout__main {
  padding: 1.25rem;
}
</style>