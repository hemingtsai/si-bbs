<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'

import { commentsApi, projectsApi, ratingsApi } from '../../api'
import { apiError } from '../../lib/errors'
import type { Comment, Project, RatingSummary } from '../../api/types'
import MarkdownView from '../../components/MarkdownView.vue'
import ReportButton from '../../components/ReportButton.vue'
import { useAuthStore } from '../../stores/auth'

const route = useRoute()
const router = useRouter()
const auth = useAuthStore()

const projectId = Number(route.params.id)
const project = ref<Project | null>(null)
const summary = ref<RatingSummary | null>(null)
const comments = ref<Comment[]>([])
const commentsPage = ref(1)
const commentsTotal = ref(0)
const moreCommentsError = ref('')
const COMMENT_PAGE_SIZE = 20
const loading = ref(true)
const error = ref('')

const score = ref(8)
const ratingComment = ref('')
const ratingError = ref('')
const newComment = ref('')
const commentError = ref('')

const isOwner = computed(() => auth.isAuthenticated && auth.userId === project.value?.submitted_by)

async function load(): Promise<void> {
  loading.value = true
  error.value = ''
  try {
    const projectRes = await projectsApi.detail(projectId)
    project.value = projectRes.data
    // 评分和评论接口对未上架（pending/rejected）项目返回 404，
    // 因此只有已通过审核的项目才去加载这两块。
    if (projectRes.data.status === 'approved') {
      const [summaryRes, commentsRes] = await Promise.all([
        ratingsApi.summary(projectId),
        commentsApi.list(projectId, { page: 1, per_page: COMMENT_PAGE_SIZE }),
      ])
      summary.value = summaryRes.data
      comments.value = commentsRes.data.items
      commentsPage.value = 1
      commentsTotal.value = commentsRes.data.total
    }
  } catch {
    error.value = '项目不存在或已下架'
  } finally {
    loading.value = false
  }
}

async function loadMoreComments(): Promise<void> {
  moreCommentsError.value = ''
  try {
    const next = commentsPage.value + 1
    const { data } = await commentsApi.list(projectId, {
      page: next,
      per_page: COMMENT_PAGE_SIZE,
    })
    comments.value = [...comments.value, ...data.items]
    commentsPage.value = next
    commentsTotal.value = data.total
  } catch (err: unknown) {
    moreCommentsError.value = apiError(err, '加载更多评论失败')
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
    commentsTotal.value += 1
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
  <div class="page" v-if="loading">加载中…</div>
  <div class="page" v-else-if="error">
    <div class="page-head">
      <div class="title"><h1>出错了</h1></div>
    </div>
    <p class="error">{{ error }}</p>
    <RouterLink to="/projects" class="btn">返回列表</RouterLink>
  </div>

  <div class="page" v-else-if="project">
    <div class="page-head">
      <div class="title">
        <h1>{{ project.name }}</h1>
        <span class="sub">{{ project.owner }}/{{ project.repo }}</span>
      </div>
      <div class="page-head-actions">
        <a class="btn" :href="project.github_url" target="_blank" rel="noopener noreferrer">GitHub</a>
        <button v-if="isOwner || auth.isStaff" class="btn btn-danger" @click="removeProject">删除</button>
        <ReportButton target-kind="project" :target-id="project.id" />
      </div>
    </div>

    <p v-if="project.description" class="section-hint" style="margin-bottom: 16px">{{ project.description }}</p>

    <div class="kv">
      <div class="kv-row"><span class="kv-key">语言</span><span class="kv-val">{{ project.language ?? '—' }}</span></div>
      <div class="kv-row"><span class="kv-key">Star / Fork</span><span class="kv-val mono">★ {{ project.stars }} · ⑂ {{ project.forks }}</span></div>
      <div class="kv-row"><span class="kv-key">License</span><span class="kv-val">{{ project.license ?? '—' }}</span></div>
      <div class="kv-row"><span class="kv-key">分类</span><span class="kv-val">{{ project.category }}</span></div>
      <div class="kv-row"><span class="kv-key">状态</span><span class="kv-val"><span class="status" :class="'status-' + project.status">{{ project.status }}</span></span></div>
      <div class="kv-row"><span class="kv-key">主题</span><span class="kv-val">
        <span v-for="topic in project.topics" :key="topic" class="tag">{{ topic }}</span>
      </span></div>
    </div>

    <p v-if="project.status !== 'approved'" class="meta" style="margin-top: 16px">
      该项目尚未通过审核，评分和评论在其上架前不可用。当前状态：
      <span class="status" :class="'status-' + project.status">{{ project.status }}</span>
    </p>

    <div v-if="project.status === 'approved'" class="section">
      <div class="section-title">评分</div>
      <p class="section-hint">
        平均 <span class="mono">{{ summary?.average ?? 0 }}</span> / 10（<span class="mono">{{ summary?.count ?? 0 }}</span> 人）
      </p>
      <form v-if="auth.isAuthenticated" class="row gap" @submit.prevent="submitRating">
        <input v-model.number="score" type="number" min="1" max="10" required style="width: 72px" />
        <input v-model="ratingComment" type="text" placeholder="一句话评价（可选）" class="grow" />
        <button type="submit" class="btn">打分</button>
      </form>
      <p v-else class="meta"><RouterLink to="/login">登录</RouterLink>后才能评分。</p>
      <p v-if="ratingError" class="error">{{ ratingError }}</p>
    </div>

    <div v-if="project.status === 'approved'" class="section">
      <div class="section-title">评论（{{ comments.length }} / {{ commentsTotal }}）</div>
      <div class="list">
        <div v-for="comment in comments" :key="comment.id" class="list-row" style="cursor: default">
          <div class="row-main">
            <span class="row-title">{{ comment.username }}<span class="row-sub" style="margin-left: 8px">{{ comment.created_at }}</span></span>
            <span class="row-sub">{{ comment.content }}</span>
          </div>
          <button
            v-if="auth.userId === comment.user_id || auth.isStaff"
            class="linklike"
            @click="removeComment(comment.id)"
          >
            删除
          </button>
          <ReportButton
            target-kind="comment"
            :target-id="comment.id"
            :author-id="comment.user_id"
            style="margin-left: 8px"
          />
        </div>
        <p v-if="comments.length === 0" class="meta" style="margin: 8px 0">还没有评论。</p>
        <p v-if="moreCommentsError" class="error">{{ moreCommentsError }}</p>
        <button
          v-if="comments.length < commentsTotal"
          class="btn"
          style="margin-top: 8px"
          @click="loadMoreComments"
        >
          加载更多评论（已显示 {{ comments.length }} / {{ commentsTotal }}）
        </button>
      </div>

      <form v-if="auth.isAuthenticated" class="form-stack" style="margin-top: 12px" @submit.prevent="submitComment">
        <textarea v-model="newComment" rows="3" placeholder="写下你的看法" maxlength="5000"></textarea>
        <div class="row gap">
          <button type="submit" class="btn">发表</button>
        </div>
      </form>
      <p v-else class="meta"><RouterLink to="/login">登录</RouterLink>后才能评论。</p>
      <p v-if="commentError" class="error">{{ commentError }}</p>
    </div>

    <div class="section">
      <div class="section-title">README</div>
      <MarkdownView :source="project.readme_raw" />
    </div>
  </div>
</template>

<style scoped>
.form-stack {
  display: flex;
  flex-direction: column;
  gap: 8px;
  max-width: 36rem;
}
.grow {
  flex: 1;
}
.list-row {
  cursor: default;
}
.list-row:hover {
  background: none;
}
</style>
