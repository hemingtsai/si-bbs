<script setup lang="ts">
import { onMounted, ref } from 'vue'

import { projectsApi } from '../api'
import type { Project } from '../api/types'
import { apiError } from '../lib/errors'

const pending = ref<Project[]>([])
const total = ref(0)
const loading = ref(true)
const error = ref('')
const note = ref('')

async function load(): Promise<void> {
  loading.value = true
  error.value = ''
  try {
    const { data } = await projectsApi.reviewQueue()
    pending.value = data.items
    total.value = data.total
  } catch (err: unknown) {
    error.value = apiError(err, '加载失败')
  } finally {
    loading.value = false
  }
}

async function review(project: Project, action: 'approve' | 'reject'): Promise<void> {
  try {
    await projectsApi.review(project.id, action, note.value || undefined)
    note.value = ''
    await load()
  } catch (err: unknown) {
    error.value = apiError(err, '审核失败')
  }
}

onMounted(load)
</script>

<template>
  <section>
    <h1>审核队列（{{ total }}）</h1>
    <p v-if="loading">加载中…</p>
    <p v-else-if="error" class="error">{{ error }}</p>
    <p v-else-if="pending.length === 0">没有待审核的项目。</p>

    <ul v-else class="queue">
      <li v-for="project in pending" :key="project.id">
        <strong>{{ project.name }}</strong>
        <span class="meta">{{ project.owner }}/{{ project.repo }} · {{ project.category }}</span>
        <p v-if="project.description">{{ project.description }}</p>
        <div class="actions">
          <input v-model="note" type="text" placeholder="备注（驳回必填）" />
          <button @click="review(project, 'approve')">通过</button>
          <button class="danger" @click="review(project, 'reject')">驳回</button>
        </div>
      </li>
    </ul>
  </section>
</template>

<style scoped>
.error {
  color: #b00020;
}
.queue {
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
.actions {
  display: flex;
  gap: 0.5rem;
  margin-top: 0.5rem;
}
.danger {
  color: #b00020;
}
</style>
