<script setup lang="ts">
import { onMounted, ref } from 'vue'

import { trashApi } from '../api'
import type { TrashItem } from '../api/types'
import { apiError } from '../lib/errors'
import { useAuthStore } from '../stores/auth'

const auth = useAuthStore()
const items = ref<TrashItem[]>([])
const loading = ref(true)
const error = ref('')

const kindLabels: Record<TrashItem['kind'], string> = {
  wiki: 'Wiki 页面',
  project: '项目',
  comment: '评论',
}

async function load(): Promise<void> {
  loading.value = true
  error.value = ''
  try {
    const { data } = await trashApi.list()
    items.value = data
  } catch (err: unknown) {
    error.value = apiError(err, '加载失败')
  } finally {
    loading.value = false
  }
}

async function restore(item: TrashItem): Promise<void> {
  try {
    await trashApi.restore(item.kind, item.id)
    await load()
  } catch (err: unknown) {
    error.value = apiError(err, '恢复失败')
  }
}

async function purge(item: TrashItem): Promise<void> {
  if (!confirm('彻底删除且不可恢复？')) return
  try {
    await trashApi.purge(item.kind, item.id)
    await load()
  } catch (err: unknown) {
    error.value = apiError(err, '删除失败')
  }
}

onMounted(load)
</script>

<template>
  <div class="page">
    <div class="page-head">
      <div class="title">
        <h1>回收站</h1>
        <span class="sub">{{ items.length }} 个已删除条目</span>
      </div>
    </div>

    <p v-if="loading" class="meta">加载中…</p>
    <p v-else-if="error" class="error">{{ error }}</p>
    <p v-else-if="items.length === 0" class="meta">回收站是空的。</p>

    <div v-else class="list">
      <div v-for="item in items" :key="`${item.kind}-${item.id}`" class="list-row" style="cursor: default">
        <div class="row-main">
          <span class="row-title">{{ item.name ?? `#${item.id}` }}</span>
          <span class="row-sub">
            {{ kindLabels[item.kind] }} · {{ item.deleted_by_username ?? '未知用户' }} 于 {{ item.deleted_at }} 删除
          </span>
        </div>
        <button class="btn" @click="restore(item)">恢复</button>
        <button v-if="auth.isAdmin" class="btn btn-danger" @click="purge(item)">彻底删除</button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.list-row:hover {
  background: none;
}
</style>
