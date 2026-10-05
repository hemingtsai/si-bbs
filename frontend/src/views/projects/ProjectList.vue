<script setup lang="ts">
import { onMounted, reactive, ref } from 'vue'

import { projectsApi } from '../../api'
import type { Project } from '../../api/types'

const projects = ref<Project[]>([])
const total = ref(0)
const page = ref(1)
const perPage = 20
const loading = ref(true)
const error = ref('')

const filters = reactive({
  category: '',
  q: '',
  sort: 'stars' as 'stars' | 'recent' | 'name',
})

async function load(): Promise<void> {
  loading.value = true
  error.value = ''
  try {
    const { data } = await projectsApi.list({
      category: filters.category || undefined,
      q: filters.q || undefined,
      sort: filters.sort,
      page: page.value,
      per_page: perPage,
    })
    projects.value = data.items
    total.value = data.total
  } catch {
    error.value = '加载失败，请重试'
  } finally {
    loading.value = false
  }
}

function refine(): void {
  page.value = 1
  load()
}

onMounted(load)
</script>

<template>
  <section>
    <h1>项目索引</h1>

    <form class="filters" @submit.prevent="refine">
      <input v-model="filters.q" type="search" placeholder="搜索名称或描述" />
      <input v-model="filters.category" type="text" placeholder="分类" />
      <select v-model="filters.sort" @change="refine">
        <option value="stars">最多星</option>
        <option value="recent">最新</option>
        <option value="name">名称</option>
      </select>
      <button type="submit">筛选</button>
      <RouterLink to="/projects/submit">提交项目</RouterLink>
    </form>

    <p v-if="loading">加载中…</p>
    <p v-else-if="error" class="error">{{ error }}</p>
    <p v-else-if="projects.length === 0">还没有通过审核的项目。</p>

    <ul v-else class="project-list">
      <li v-for="project in projects" :key="project.id">
        <RouterLink :to="{ name: 'project-detail', params: { id: project.id } }">
          {{ project.name }}
        </RouterLink>
        <span class="meta">
          {{ project.owner }}/{{ project.repo }}
          <template v-if="project.language"> · {{ project.language }}</template>
          · ★ {{ project.stars }} · ⑂ {{ project.forks }}
        </span>
        <p v-if="project.description" class="description">{{ project.description }}</p>
        <span v-for="topic in project.topics" :key="topic" class="topic">{{ topic }}</span>
      </li>
    </ul>

    <nav v-if="total > perPage" class="pager">
      <button :disabled="page <= 1" @click="page--; load()">上一页</button>
      <span>{{ page }} / {{ Math.ceil(total / perPage) }}</span>
      <button :disabled="page * perPage >= total" @click="page++; load()">下一页</button>
    </nav>
  </section>
</template>

<style scoped>
.filters {
  display: flex;
  gap: 0.5rem;
  flex-wrap: wrap;
  margin-bottom: 1rem;
}
.error {
  color: #b00020;
}
.project-list {
  list-style: none;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: 1rem;
}
.meta {
  margin-left: 0.5rem;
  color: var(--si-muted, #666);
  font-size: 0.9rem;
}
.description {
  margin: 0.25rem 0;
  color: var(--si-muted, #444);
}
.topic {
  display: inline-block;
  margin-right: 0.4rem;
  padding: 0 0.5rem;
  border-radius: 1rem;
  background: var(--si-border, #eee);
  font-size: 0.8rem;
}
.pager {
  margin-top: 1rem;
  display: flex;
  gap: 1rem;
  align-items: center;
}
</style>