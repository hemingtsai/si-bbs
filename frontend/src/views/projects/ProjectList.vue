<script setup lang="ts">
import { onMounted, reactive, ref } from 'vue'

import { projectsApi } from '../../api'
import type { ProjectSummary } from '../../api/types'
import { PROJECT_CATEGORIES } from '../../lib/categories'

const projects = ref<ProjectSummary[]>([])
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

const statusDot = (status: string) => (status === 'approved' ? 'dot dot-ok' : status === 'pending' ? 'dot dot-warn' : 'dot dot-danger')

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
  <div class="page">
    <div class="page-head">
      <div class="title">
        <h1>项目索引</h1>
        <span class="sub">{{ total }} 个已收项目</span>
      </div>
      <div class="page-head-actions">
        <RouterLink to="/projects/submit" class="btn">提交项目</RouterLink>
      </div>
    </div>

    <form class="controls" @submit.prevent="refine">
      <label class="field">
        <span class="field-label">搜索</span>
        <input v-model="filters.q" type="search" placeholder="名称或描述" />
      </label>
      <label class="field">
        <span class="field-label">分类</span>
        <select v-model="filters.category" @change="refine">
          <option value="">全部</option>
          <option v-for="c in PROJECT_CATEGORIES" :key="c" :value="c">{{ c }}</option>
        </select>
      </label>
      <label class="field">
        <span class="field-label">排序</span>
        <select v-model="filters.sort" @change="refine">
          <option value="stars">最多星</option>
          <option value="recent">最新</option>
          <option value="name">名称</option>
        </select>
      </label>
      <div class="field field-actions">
        <button type="submit" class="btn">筛选</button>
      </div>
    </form>

    <p v-if="loading" class="meta">加载中…</p>
    <p v-else-if="error" class="error">{{ error }}</p>
    <p v-else-if="projects.length === 0" class="meta">还没有通过审核的项目。</p>

    <div v-else class="list">
      <RouterLink
        v-for="project in projects"
        :key="project.id"
        class="list-row"
        :to="{ name: 'project-detail', params: { id: project.id } }"
      >
        <div class="row-main">
          <span class="row-title">
            <span :class="statusDot(project.status)"></span>
            {{ project.name }}
          </span>
          <span class="row-sub">
            {{ project.owner }}/{{ project.repo }}
            <template v-if="project.language"> · {{ project.language }}</template>
            <template v-if="project.description"> · {{ project.description }}</template>
          </span>
          <span class="row-sub" v-if="project.topics.length">
            <span v-for="topic in project.topics" :key="topic" class="tag">{{ topic }}</span>
          </span>
        </div>
        <span class="col-num">★ {{ project.stars }}</span>
        <span class="col-num">⑂ {{ project.forks }}</span>
      </RouterLink>
    </div>

    <nav v-if="total > perPage" class="pager">
      <button class="btn" :disabled="page <= 1" @click="page--; load()">上一页</button>
      <span class="mono">{{ page }} / {{ Math.ceil(total / perPage) }}</span>
      <button class="btn" :disabled="page * perPage >= total" @click="page++; load()">下一页</button>
    </nav>
  </div>
</template>

<style scoped>
.field-actions {
  justify-content: flex-end;
  flex-direction: row;
  align-items: flex-end;
}
</style>
