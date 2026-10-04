<script setup lang="ts">
import { RouterLink, RouterView } from 'vue-router'
</script>

<template>
  <div class="layout">
    <header class="layout__header">
      <RouterLink to="/" class="layout__brand">SI BBS</RouterLink>
      <nav class="layout__nav">
        <RouterLink to="/wiki">Wiki</RouterLink>
        <RouterLink to="/projects">项目</RouterLink>
        <RouterLink to="/login">登录</RouterLink>
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
}
.layout__main {
  padding: 1.25rem;
}
</style>