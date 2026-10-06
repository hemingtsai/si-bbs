<script setup lang="ts">
import { onMounted, ref } from 'vue'

import { projectsApi, wikiApi } from '../api'
import type { ProjectSummary, WikiPageSummary } from '../api/types'

const projects = ref<ProjectSummary[]>([])
const pages = ref<WikiPageSummary[]>([])
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
  <div class="page">
    <div class="page-head">
      <div class="title">
        <h1>我的</h1>
      </div>
    </div>

    <p v-if="loading" class="meta">加载中…</p>

    <div class="section">
      <div class="section-title">我提交的项目</div>
      <p v-if="projects.length === 0" class="section-hint">还没有提交过项目。</p>
      <div v-else class="list">
        <RouterLink
          v-for="project in projects"
          :key="project.id"
          class="list-row"
          :to="{ name: 'project-detail', params: { id: project.id } }"
        >
          <div class="row-main">
            <span class="row-title">
              <span class="dot" :class="project.status === 'approved' ? 'dot-ok' : project.status === 'pending' ? 'dot-warn' : 'dot-danger'"></span>
              {{ project.name }}
            </span>
            <span class="row-sub">{{ project.status }} · ★ {{ project.stars }}</span>
          </div>
        </RouterLink>
      </div>
    </div>

    <div class="section">
      <div class="section-title">我的 Wiki 页面</div>
      <p v-if="pages.length === 0" class="section-hint">还没有创建过页面。</p>
      <div v-else class="list">
        <RouterLink
          v-for="page in pages"
          :key="page.id"
          class="list-row"
          :to="{ name: 'wiki-detail', params: { slug: page.slug } }"
        >
          <div class="row-main">
            <span class="row-title">
              <span class="dot" :class="page.status === 'published' ? 'dot-ok' : ''"></span>
              {{ page.title }}
            </span>
            <span class="row-sub">{{ page.status === 'draft' ? '草稿' : '已发布' }} · {{ page.category }}</span>
          </div>
        </RouterLink>
      </div>
    </div>
  </div>
</template>
