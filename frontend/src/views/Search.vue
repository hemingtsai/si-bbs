<script setup lang="ts">
import { onMounted, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'

import { searchApi } from '../api'
import type { SearchHit } from '../api/types'

const route = useRoute()
const router = useRouter()

const term = ref(String(route.query.q ?? ''))
const kind = ref<'all' | 'wiki' | 'forum' | 'project'>('all')
const hits = ref<SearchHit[]>([])
const total = ref(0)
const loading = ref(false)
const error = ref('')

const kindLabel: Record<SearchHit['kind'], string> = {
  wiki: 'Wiki',
  forum: '论坛',
  project: '项目',
}

/// Where a hit leads. Wiki pages are addressed by slug, everything else by id.
function targetOf(hit: SearchHit): { name: string; params: Record<string, string | number> } {
  switch (hit.kind) {
    case 'wiki':
      return { name: 'wiki-detail', params: { slug: hit.slug ?? String(hit.id) } }
    case 'forum':
      return { name: 'forum-detail', params: { id: hit.id } }
    default:
      return { name: 'project-detail', params: { id: hit.id } }
  }
}

async function run(): Promise<void> {
  const q = term.value.trim()
  if (!q) {
    hits.value = []
    total.value = 0
    return
  }
  loading.value = true
  error.value = ''
  try {
    const { data } = await searchApi.query({ q, kind: kind.value })
    hits.value = data.items
    total.value = data.total
  } catch {
    error.value = '搜索失败，请稍后再试'
  } finally {
    loading.value = false
  }
}

/// Keep the URL in step with the query so a result page can be shared or reloaded.
function submit(): void {
  router.replace({ name: 'search', query: { q: term.value.trim() || undefined } })
}

watch(
  () => route.query.q,
  (q) => {
    term.value = String(q ?? '')
    void run()
  },
)

onMounted(run)
</script>

<template>
  <div class="page">
    <div class="page-head">
      <div class="title">
        <h1>搜索</h1>
        <span class="sub" v-if="total > 0">找到 {{ total }} 条</span>
      </div>
    </div>

    <form class="controls" @submit.prevent="submit">
      <label class="field grow">
        <span class="field-label">关键词</span>
        <input v-model="term" type="search" placeholder="帖子、Wiki、项目…" autofocus />
      </label>
      <label class="field">
        <span class="field-label">范围</span>
        <select v-model="kind" @change="run">
          <option value="all">全部</option>
          <option value="forum">论坛</option>
          <option value="wiki">Wiki</option>
          <option value="project">项目</option>
        </select>
      </label>
      <div class="field field-actions">
        <button type="submit" class="btn btn-primary">搜索</button>
      </div>
    </form>

    <p class="meta">
      支持中文子串（例如"量化"）；少于 3 个字符时按子串匹配，3 个字符起按全文检索并按相关度排序。
    </p>

    <p v-if="loading" class="meta">搜索中…</p>
    <p v-else-if="error" class="error">{{ error }}</p>
    <p v-else-if="term.trim() && hits.length === 0" class="meta">没有匹配的结果。</p>

    <div v-else class="list">
      <RouterLink
        v-for="hit in hits"
        :key="`${hit.kind}-${hit.id}`"
        class="list-row"
        :to="targetOf(hit)"
      >
        <div class="row-main">
          <span class="row-title">
            <span class="tag">{{ kindLabel[hit.kind] }}</span>
            {{ hit.title }}
          </span>
          <span class="row-sub">{{ hit.excerpt }}</span>
        </div>
        <span class="row-num mono">{{ hit.updated_at }}</span>
      </RouterLink>
    </div>
  </div>
</template>

<style scoped>
.controls {
  display: flex;
  align-items: flex-end;
  gap: 12px;
  flex-wrap: wrap;
  margin-bottom: 8px;
}
.grow {
  flex: 1 1 260px;
}
.field-actions {
  flex-direction: row;
  align-items: flex-end;
}
.row-num {
  color: var(--text-2);
  font-size: 13px;
  white-space: nowrap;
}
</style>
