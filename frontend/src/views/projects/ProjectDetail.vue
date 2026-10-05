<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'

import { commentsApi, projectsApi, ratingsApi } from '../../api'
import type { Comment, Project, RatingSummary } from '../../api/types'
import MarkdownView from '../../components/MarkdownView.vue'
import { useAuthStore } from '../../stores/auth'

const route = useRoute()
const router = useRouter()
const auth = useAuthStore()

const projectId = Number(route.params.id)
const project = ref<Project | null>(null)
const summary = ref<RatingSummary | null>(null)
const comments = ref<Comment[]>([])
const loading = ref(true)
const error = ref('')

const score = ref(8)
const ratingComment = ref('')
const ratingError = ref('')
const newComment = ref('')
const commentError = ref('')

async function load(): Promise<void> {
  loading.value = true
  error.value = ''
  try {
    const [projectRes, summaryRes, commentsRes] = await Promise.all([
      projectsApi.detail(projectId),
      ratingsApi.summary(projectId),
      commentsApi.list(projectId, { per_page: 50 }),
    ])
    project.value = projectRes.data
    summary.value = summaryRes.data
    comments.value = commentsRes.data.items
  } catch {
    error.value = '项目不存在或已下架'
  } finally {
    loading.value = false
  }
}

async function submitRating(): Promise<void> {
  ratingError.value = ''
  try {
    const { data } = await ratingsApi.rate(projectId, score.value, ratingComment.value || undefined)
    summary.value = { project_id: data.project_id, average: data.average, count: data.count }
  } catch (err: unknown) {
    ratingError.value =
      (err as { response?: { data?: { error?: string } } })?.response?.data?.error ??
      '评分失败'
  }
}

async function submitComment(): Promise<void> {
  commentError.value = ''
  const content = newComment.value.trim()
  if (!content) {
    commentError.value = '评论不能为空'
    return
  }
  try {
    const { data } = await commentsApi.create(projectId, content)
    comments.value.push(data)
    newComment.value = ''
  } catch (err: unknown) {
    commentError.value =
      (err as { response?: { data?: { error?: string } } })?.response?.data?.error ??
      '评论失败'
  }
}

async function removeComment(commentId: number): Promise<void> {
  if (!confirm('删除这条评论？')) return
  await commentsApi.remove(projectId, commentId)
  comments.value = comments.value.filter((c) => c.id !== commentId)
}

async function removeProject(): Promise<void> {
  if (!confirm('删除该项目？可在回收站恢复。')) return
  await projectsApi.remove(projectId)
  router.push({ name: 'project-list' })
}

onMounted(load)
</script>

<template>
  <section v-if="loading">加载中…</section>
  <section v-else-if="error">
    <p class="error">{{ error }}</p>
    <RouterLink to="/projects">返回列表</RouterLink>
  </section>

  <section v-else-if="project">
    <header class="project__header">
      <h1>{{ project.name }}</h1>
      <a :href="project.github_url" target="_blank" rel="noopener noreferrer">GitHub ↗</a>
    </header>

    <p v-if="project.description">{{ project.description }}</p>

    <dl class="meta">
      <dt>语言</dt>
      <dd>{{ project.language ?? '—' }}</dd>
      <dt>Star</dt>
      <dd>{{ project.stars }}</dd>
      <dt>Fork</dt>
      <dd>{{ project.forks }}</dd>
      <dt>License</dt>
      <dd>{{ project.license ?? '—' }}</dd>
      <dt>分类</dt>
      <dd>{{ project.category }}</dd>
      <dt>状态</dt>
      <dd>{{ project.status }}</dd>
    </dl>

    <div class="topics">
      <span v-for="topic in project.topics" :key="topic" class="topic">{{ topic }}</span>
    </div>

    <section class="rating">
      <h2>评分</h2>
      <p v-if="summary">平均 {{ summary.average }} / 10（{{ summary.count }} 人）</p>
      <form v-if="auth.isAuthenticated" class="rating__form" @submit.prevent="submitRating">
        <label>
          分数（1–10）
          <input v-model.number="score" type="number" min="1" max="10" required />
        </label>
        <input v-model="ratingComment" type="text" placeholder="一句话评价（可选）" />
        <button type="submit">打分</button>
      </form>
      <p v-else><RouterLink to="/login">登录</RouterLink>后才能评分。</p>
      <p v-if="ratingError" class="error">{{ ratingError }}</p>
    </section>

    <section class="comments">
      <h2>评论</h2>
      <ul class="comments__list">
        <li v-for="comment in comments" :key="comment.id">
          <strong>{{ comment.username }}</strong>
          <span class="comments__time">{{ comment.created_at }}</span>
          <p>{{ comment.content }}</p>
          <button
            v-if="auth.username === comment.username || auth.isStaff"
            class="linklike"
            @click="removeComment(comment.id)"
          >
            删除
          </button>
        </li>
        <li v-if="comments.length === 0">还没有评论。</li>
      </ul>

      <form v-if="auth.isAuthenticated" class="comments__form" @submit.prevent="submitComment">
        <textarea v-model="newComment" rows="3" placeholder="写下你的看法" maxlength="5000" />
        <button type="submit">发表</button>
      </form>
      <p v-else><RouterLink to="/login">登录</RouterLink>后才能评论。</p>
      <p v-if="commentError" class="error">{{ commentError }}</p>
    </section>

    <MarkdownView :source="project.readme_raw" />

    <button v-if="auth.isAuthenticated && auth.userId === project.submitted_by" class="danger" @click="removeProject">删除项目</button>
  </section>
</template>

<style scoped>
.error {
  color: #b00020;
}
.project__header {
  display: flex;
  align-items: baseline;
  gap: 1rem;
}
.meta {
  display: grid;
  grid-template-columns: auto 1fr;
  gap: 0 1rem;
}
.topic {
  display: inline-block;
  margin-right: 0.4rem;
  padding: 0 0.5rem;
  border-radius: 1rem;
  background: var(--si-border, #eee);
  font-size: 0.8rem;
}
.linklike {
  border: none;
  background: none;
  padding: 0;
  color: #b00020;
  cursor: pointer;
}
.danger {
  margin-top: 1rem;
  color: #b00020;
}
.comments__list {
  list-style: none;
  padding: 0;
}
.comments__time {
  color: var(--si-muted, #666);
  font-size: 0.85rem;
  margin-left: 0.5rem;
}
.rating__form,
.comments__form {
  display: flex;
  flex-direction: column;
  gap: 0.5rem;
  max-width: 32rem;
}
</style>