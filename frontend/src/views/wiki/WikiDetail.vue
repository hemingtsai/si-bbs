<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'

import { wikiApi } from '../../api'
import type { WikiPage } from '../../api/types'
import MarkdownView from '../../components/MarkdownView.vue'
import { useAuthStore } from '../../stores/auth'

const route = useRoute()
const router = useRouter()
const auth = useAuthStore()

const page = ref<WikiPage | null>(null)
const loading = ref(true)
const error = ref('')

const slug = computed(() => String(route.params.slug))

async function load(): Promise<void> {
  loading.value = true
  error.value = ''
  try {
    const { data } = await wikiApi.detail(slug.value)
    page.value = data
  } catch {
    error.value = '页面不存在'
  } finally {
    loading.value = false
  }
}

const canEdit = computed(() => {
  if (!page.value || !auth.isAuthenticated) return false
  return auth.userId === page.value.author_id || auth.isStaff
})

async function remove(): Promise<void> {
  if (!page.value || !confirm('删除该页面？可在回收站恢复。')) return
  await wikiApi.remove(page.value.id)
  router.push({ name: 'wiki-list' })
}

onMounted(load)
</script>

<template>
  <section v-if="loading">加载中…</section>
  <section v-else-if="error">
    <p class="error">{{ error }}</p>
    <RouterLink to="/wiki">返回列表</RouterLink>
  </section>

  <section v-else-if="page">
    <header class="wiki__header">
      <h1>{{ page.title }}</h1>
      <RouterLink v-if="canEdit" :to="{ name: 'wiki-edit', params: { slug: page.slug } }">编辑</RouterLink>
      <button v-if="canEdit" class="danger" @click="remove">删除</button>
    </header>

    <p class="meta">
      {{ page.category }} · {{ page.author_username ?? '未知作者' }} ·
      {{ page.status === 'draft' ? '草稿' : '已发布' }} · 更新于 {{ page.updated_at }}
    </p>

    <MarkdownView :source="page.content" />
  </section>
</template>

<style scoped>
.error {
  color: #b00020;
}
.wiki__header {
  display: flex;
  align-items: baseline;
  gap: 1rem;
}
.meta {
  color: var(--si-muted, #666);
  font-size: 0.9rem;
}
.danger {
  color: #b00020;
}
</style>