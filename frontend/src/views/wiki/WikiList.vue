<script setup lang="ts">
import { onMounted, ref } from 'vue'

import { wikiApi } from '../../api'
import type { WikiCategory, WikiPage } from '../../api/types'
import { useAuthStore } from '../../stores/auth'

const auth = useAuthStore()
const pages = ref<WikiPage[]>([])
const categories = ref<WikiCategory[]>([])
const total = ref(0)
const page = ref(1)
const perPage = 20
const q = ref('')
const category = ref('')
const loading = ref(true)
const error = ref('')

async function load(): Promise<void> {
  loading.value = true
  error.value = ''
  try {
    const [pagesRes, categoriesRes] = await Promise.all([
      wikiApi.list({ q: q.value || undefined, category: category.value || undefined, page: page.value }),
      wikiApi.categories(),
    ])
    pages.value = pagesRes.data.items
    total.value = pagesRes.data.total
    categories.value = categoriesRes.data
  } catch {
    error.value = '加载失败，请重试'
  } finally {
    loading.value = false
  }
}

function filter(): void {
  page.value = 1
  load()
}

onMounted(load)
</script>

<template>
  <section>
    <h1>Wiki 知识库</h1>

    <form class="filters" @submit.prevent="filter">
      <input v-model="q" type="search" placeholder="搜索标题或正文" />
      <select v-model="category" @change="filter">
        <option value="">全部分類</option>
        <option v-for="c in categories" :key="c.category" :value="c.category">
          {{ c.category }}（{{ c.count }}）
        </option>
      </select>
      <button type="submit">搜索</button>
      <RouterLink v-if="auth.isAuthenticated" to="/wiki/new">新建页面</RouterLink>
    </form>

    <p v-if="loading">加载中…</p>
    <p v-else-if="error" class="error">{{ error }}</p>
    <p v-else-if="pages.length === 0">没有匹配的页面。</p>

    <ul v-else class="page-list">
      <li v-for="pageItem in pages" :key="pageItem.id">
        <RouterLink :to="{ name: 'wiki-detail', params: { slug: pageItem.slug } }">
          {{ pageItem.title }}
        </RouterLink>
        <span class="meta">
          {{ pageItem.category }} ·
          {{ pageItem.author_username ?? '未知作者' }} ·
          {{ pageItem.updated_at }}
        </span>
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
.page-list {
  list-style: none;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: 0.75rem;
}
.meta {
  margin-left: 0.5rem;
  color: var(--si-muted, #666);
  font-size: 0.9rem;
}
.pager {
  margin-top: 1rem;
  display: flex;
  gap: 1rem;
  align-items: center;
}
</style>