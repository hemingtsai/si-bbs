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
  <div class="page">
    <div class="page-head">
      <div class="title">
        <h1>审核队列</h1>
        <span class="sub">{{ total }} 个待审核</span>
      </div>
    </div>

    <p v-if="loading" class="meta">加载中…</p>
    <p v-else-if="error" class="error">{{ error }}</p>
    <p v-else-if="pending.length === 0" class="meta">没有待审核的项目。</p>

    <div v-else class="list">
      <div v-for="project in pending" :key="project.id" class="list-row" style="cursor: default; align-items: flex-start">
        <div class="row-main">
          <span class="row-title">
            <span class="dot dot-warn"></span>{{ project.name }}
          </span>
          <span class="row-sub">{{ project.owner }}/{{ project.repo }} · {{ project.category }}</span>
          <span v-if="project.description" class="row-sub">{{ project.description }}</span>
          <div class="row gap" style="margin-top: 8px">
            <input v-model="note" type="text" placeholder="备注（驳回必填）" style="flex: 1; max-width: 16rem" />
            <button class="btn" @click="review(project, 'approve')">通过</button>
            <button class="btn btn-danger" @click="review(project, 'reject')">驳回</button>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.list-row:hover {
  background: none;
}
</style>
