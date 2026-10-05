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
  <section>
    <h1>回收站</h1>
    <p v-if="loading">加载中…</p>
    <p v-else-if="error" class="error">{{ error }}</p>
    <p v-else-if="items.length === 0">回收站是空的。</p>

    <ul v-else class="trash">
      <li v-for="item in items" :key="`${item.kind}-${item.id}`">
        <span class="kind">{{ kindLabels[item.kind] }}</span>
        <span class="name">{{ item.name ?? `#${item.id}` }}</span>
        <span class="meta">
          {{ item.deleted_by_username ?? '未知用户' }} 于 {{ item.deleted_at }} 删除
        </span>
        <button @click="restore(item)">恢复</button>
        <button v-if="auth.isAdmin" class="danger" @click="purge(item)">彻底删除</button>
      </li>
    </ul>
  </section>
</template>

<style scoped>
.error {
  color: #b00020;
}
.trash {
  list-style: none;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: 0.5rem;
}
.trash li {
  display: flex;
  gap: 0.75rem;
  align-items: baseline;
}
.kind {
  font-size: 0.8rem;
  color: var(--si-muted, #666);
}
.meta {
  color: var(--si-muted, #666);
  font-size: 0.85rem;
  margin-right: auto;
}
.danger {
  color: #b00020;
}
</style>
