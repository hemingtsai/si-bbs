<script setup lang="ts">
import { onMounted, ref } from 'vue'

import { projectsApi, reportApi } from '../api'
import type { AdminReport, ProjectSummary } from '../api/types'
import { apiError } from '../lib/errors'

const tab = ref<'reports' | 'projects'>('reports')

const reports = ref<AdminReport[]>([])
const reportFilter = ref<'open' | 'all'>('open')
const reportTotal = ref(0)
const reportNote = ref('')
const reportError = ref('')

const kindLabel: Record<AdminReport['target_kind'], string> = {
  forum_post: '论坛帖子',
  forum_comment: '论坛回复',
  wiki: 'Wiki 页面',
  project: '项目',
  comment: '项目评论',
}

async function loadReports(): Promise<void> {
  reportError.value = ''
  try {
    const { data } = await reportApi.queue({ status: reportFilter.value })
    reports.value = data.items
    reportTotal.value = data.total
  } catch (err: unknown) {
    reportError.value = apiError(err, '无法加载举报队列')
  }
}

async function resolve(report: AdminReport, status: 'resolved' | 'dismissed'): Promise<void> {
  reportError.value = ''
  try {
    await reportApi.resolve(report.id, { status, note: reportNote.value.trim() || undefined })
    reportNote.value = ''
    await loadReports()
  } catch (err: unknown) {
    reportError.value = apiError(err, '处理失败')
  }
}

const pending = ref<ProjectSummary[]>([])
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

async function review(project: ProjectSummary, action: 'approve' | 'reject'): Promise<void> {
  try {
    await projectsApi.review(project.id, action, note.value || undefined)
    note.value = ''
    await load()
  } catch (err: unknown) {
    error.value = apiError(err, '审核失败')
  }
}

onMounted(async () => {
  await Promise.all([load(), loadReports()])
})
</script>

<template>
  <div class="page">
    <div class="page-head">
      <div class="title">
        <h1>审核</h1>
        <span class="sub">举报 {{ reportTotal }} 条 · 项目 {{ total }} 个待审核</span>
      </div>
      <div class="page-head-actions">
        <button class="btn" :class="{ 'btn-primary': tab === 'reports' }" @click="tab = 'reports'">
          举报队列
        </button>
        <button class="btn" :class="{ 'btn-primary': tab === 'projects' }" @click="tab = 'projects'">
          项目审核
        </button>
      </div>
    </div>

    <section v-if="tab === 'reports'" class="section">
      <div class="controls">
        <label class="field">
          <span class="field-label">范围</span>
          <select v-model="reportFilter" @change="loadReports">
            <option value="open">未处理</option>
            <option value="all">全部</option>
          </select>
        </label>
        <label class="field grow">
          <span class="field-label">处理说明（可选）</span>
          <input v-model="reportNote" type="text" maxlength="200" placeholder="例如：已删除 / 不是广告" />
        </label>
      </div>
      <p v-if="reportError" class="error">{{ reportError }}</p>
      <p v-else-if="reports.length === 0" class="meta">
        {{ reportFilter === 'open' ? '没有待处理的举报。' : '还没有任何举报。' }}
      </p>
      <div v-else class="list">
        <div
          v-for="report in reports"
          :key="report.id"
          class="list-row"
          style="cursor: default; align-items: flex-start"
        >
          <div class="row-main">
            <span class="row-title">
              <span class="status" :class="'status-' + report.status">{{ report.status }}</span>
              {{ kindLabel[report.target_kind] }}：{{ report.target_title ?? `#${report.target_id}` }}
              <span v-if="report.target_deleted" class="row-sub">（内容已被删除）</span>
            </span>
            <span class="row-sub">
              举报人 {{ report.reporter_username ?? '未知' }} · {{ report.created_at }}
            </span>
            <span class="row-sub">理由：{{ report.reason }}</span>
            <span v-if="report.note" class="row-sub">处理说明：{{ report.note }}</span>
          </div>
          <div v-if="report.status === 'open'" class="row gap">
            <button class="linklike" @click="resolve(report, 'resolved')">标记已处理</button>
            <button class="linklike" @click="resolve(report, 'dismissed')">驳回</button>
          </div>
        </div>
      </div>
    </section>

    <p v-if="tab === 'projects' && loading" class="meta">加载中…</p>
    <p v-else-if="tab === 'projects' && error" class="error">{{ error }}</p>
    <p v-else-if="tab === 'projects' && pending.length === 0" class="meta">没有待审核的项目。</p>

    <div v-else-if="tab === 'projects'" class="list">
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
