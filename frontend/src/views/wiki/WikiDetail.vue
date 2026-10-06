<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'

import { wikiApi } from '../../api'
import type { WikiPage } from '../../api/types'
import MarkdownView from '../../components/MarkdownView.vue'
import ReportButton from '../../components/ReportButton.vue'
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
  <div class="page" v-if="loading">加载中…</div>
  <div class="page" v-else-if="error">
    <div class="page-head">
      <div class="title"><h1>出错了</h1></div>
    </div>
    <p class="error">{{ error }}</p>
    <RouterLink to="/wiki" class="btn">返回列表</RouterLink>
  </div>

  <div class="page" v-else-if="page">
    <div class="page-head">
      <div class="title">
        <h1>{{ page.title }}</h1>
        <span class="sub">{{ page.category }} · {{ page.author_username ?? '未知作者' }} · 更新于 {{ page.updated_at }}</span>
      </div>
      <div class="page-head-actions">
        <span class="status" :class="'status-' + page.status">{{ page.status === 'draft' ? '草稿' : '已发布' }}</span>
        <RouterLink class="btn" :to="{ name: 'wiki-history', params: { slug: page.slug } }">
          历史（第 {{ page.revision }} 版）
        </RouterLink>
        <RouterLink v-if="canEdit" class="btn" :to="{ name: 'wiki-edit', params: { slug: page.slug } }">编辑</RouterLink>
        <ReportButton target-kind="wiki" :target-id="page.id" :author-id="page.author_id" />
        <button v-if="canEdit" class="btn btn-danger" @click="remove">删除</button>
      </div>
    </div>

    <MarkdownView :source="page.content" />
  </div>
</template>
