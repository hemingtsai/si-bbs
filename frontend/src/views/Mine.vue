<script setup lang="ts">
import { onMounted, ref } from 'vue'

import { projectsApi, wikiApi } from '../api'
import type { Project, WikiPage } from '../api/types'
const projects = ref<Project[]>([])
const pages = ref<WikiPage[]>([])
const loading = ref(true)

async function load(): Promise<void> {
  loading.value = true
  try {
    const [p, w] = await Promise.all([projectsApi.mine(), wikiApi.mine()])
    projects.value = p.data.items
    pages.value = w.data.items
  } finally {
    loading.value = false
  }
}

onMounted(load)
</script>

<template>
  <section>
    <h1>我的</h1>
    <p v-if="loading">加载中…</p>

    <h2>我提交的项目</h2>
    <p v-if="projects.length === 0">还没有提交过项目。</p>
    <ul v-else>
      <li v-for="project in projects" :key="project.id">
        <RouterLink :to="{ name: 'project-detail', params: { id: project.id } }">
          {{ project.name }}
        </RouterLink>
        <span class="meta">{{ project.status }} · ★ {{ project.stars }}</span>
      </li>
    </ul>

    <h2>我的 Wiki 页面</h2>
    <p v-if="pages.length === 0">还没有创建过页面。</p>
    <ul v-else>
      <li v-for="page in pages" :key="page.id">
        <RouterLink :to="{ name: 'wiki-detail', params: { slug: page.slug } }">
          {{ page.title }}
        </RouterLink>
        <span class="meta">{{ page.status === 'draft' ? '草稿' : '已发布' }} · {{ page.category }}</span>
      </li>
    </ul>
  </section>
</template>

<style scoped>
.meta {
  margin-left: 0.5rem;
  color: var(--si-muted, #666);
  font-size: 0.9rem;
}
li {
  margin-bottom: 0.4rem;
}
</style>
