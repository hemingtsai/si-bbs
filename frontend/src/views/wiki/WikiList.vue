<script setup lang="ts">
import { onMounted, ref } from 'vue'

import { wikiApi } from '../../api'
import type { WikiCategory, WikiPageSummary } from '../../api/types'
import { useAuthStore } from '../../stores/auth'

const auth = useAuthStore()
const pages = ref<WikiPageSummary[]>([])
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
  <div class="page">
    <div class="page-head">
      <div class="title">
        <h1>Wiki</h1>
        <span class="sub">{{ total }} 篇已发布</span>
      </div>
      <div class="page-head-actions">
        <RouterLink v-if="auth.isAuthenticated" to="/wiki/new" class="btn">新建页面</RouterLink>
      </div>
    </div>

    <form class="controls" @submit.prevent="filter">
      <label class="field">
        <span class="field-label">搜索</span>
        <input v-model="q" type="search" placeholder="标题或正文" />
      </label>
      <label class="field">
        <span class="field-label">分类</span>
        <select v-model="category" @change="filter">
          <option value="">全部分类</option>
          <option v-for="c in categories" :key="c.category" :value="c.category">
            {{ c.category }}（{{ c.count }}）
          </option>
        </select>
      </label>
      <div class="field field-actions">
        <button type="submit" class="btn">搜索</button>
      </div>
    </form>

    <p v-if="loading" class="meta">加载中…</p>
    <p v-else-if="error" class="error">{{ error }}</p>
    <p v-else-if="pages.length === 0" class="meta">没有匹配的页面。</p>

    <div v-else class="list">
      <RouterLink
        v-for="pageItem in pages"
        :key="pageItem.id"
        class="list-row"
        :to="{ name: 'wiki-detail', params: { slug: pageItem.slug } }"
      >
        <div class="row-main">
          <span class="row-title">
            <span class="dot" :class="pageItem.status === 'published' ? 'dot-ok' : ''"></span>
            {{ pageItem.title }}
          </span>
          <span class="row-sub">
            {{ pageItem.category }} · {{ pageItem.author_username ?? '未知作者' }} · {{ pageItem.updated_at }}
          </span>
        </div>
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
