<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'

import { forumApi } from '../../api'
import type { ForumComment, ForumPost } from '../../api/types'
import MarkdownView from '../../components/MarkdownView.vue'
import ReportButton from '../../components/ReportButton.vue'
import { useAuthStore } from '../../stores/auth'
import { apiError } from '../../lib/errors'
import { withPow } from '../../lib/pow'
import { useSolving } from '../../lib/usePow'

const route = useRoute()
const router = useRouter()
const auth = useAuthStore()

const post = ref<ForumPost | null>(null)
const comments = ref<ForumComment[]>([])
/// Replies are paged: the API caps a page at 100 and silently returning the first
/// page forever made long threads look truncated.
const commentsPage = ref(1)
const commentsTotal = ref(0)
const moreCommentsError = ref('')
const loading = ref(true)
const error = ref('')
const newComment = ref('')
const solving = useSolving()
const commentError = ref('')

const postId = computed(() => Number(route.params.id))

const COMMENT_PAGE_SIZE = 20

async function loadMoreComments(): Promise<void> {
  moreCommentsError.value = ''
  try {
    const next = commentsPage.value + 1
    const { data } = await forumApi.comments(postId.value, {
      page: next,
      per_page: COMMENT_PAGE_SIZE,
    })
    // Append rather than replace: the reader is scrolling a thread, not paging a
    // table.
    comments.value = [...comments.value, ...data.items]
    commentsPage.value = next
    commentsTotal.value = data.total
  } catch (err: unknown) {
    moreCommentsError.value = apiError(err, '加载更多回复失败')
  }
}
const liked = ref(false)

const boardLabel: Record<string, string> = { models: '模型讨论', tools: '工具交流', life: '谈天说地' }

async function load(): Promise<void> {
  loading.value = true
  error.value = ''
  try {
    const [postRes, commentsRes] = await Promise.all([
      forumApi.detail(postId.value),
      forumApi.comments(postId.value, { page: 1, per_page: COMMENT_PAGE_SIZE }),
    ])
    post.value = postRes.data
    comments.value = commentsRes.data.items
    commentsPage.value = 1
    commentsTotal.value = commentsRes.data.total
  } catch {
    error.value = '帖子不存在或已删除'
  } finally {
    loading.value = false
  }
}

async function toggleLike(): Promise<void> {
  if (!post.value) return
  try {
    const { data } = await forumApi.like(postId.value)
    liked.value = data.liked
    post.value = { ...post.value, likes_count: data.likes_count }
  } catch (err: unknown) {
    error.value = apiError(err, '点赞失败')
  }
}

async function toggleCommentLike(comment: ForumComment): Promise<void> {
  try {
    const { data } = await forumApi.likeComment(comment.id)
    comment.likes_count = data.likes_count
  } catch (err: unknown) {
    apiError(err, '点赞失败')
  }
}

async function submitComment(): Promise<void> {
  commentError.value = ''
  const content = newComment.value.trim()
  if (!content) {
    commentError.value = '回复不能为空'
    return
  }
  try {
    const { data } = await withPow((pow) => forumApi.createComment(postId.value, content, pow))
    comments.value.push(data)
    commentsTotal.value += 1
    newComment.value = ''
  } catch (err: unknown) {
    commentError.value = apiError(err, '回复失败')
  }
}

async function removeComment(id: number): Promise<void> {
  if (!confirm('删除这条回复？')) return
  await forumApi.deleteComment(id)
  comments.value = comments.value.filter((c) => c.id !== id)
}

async function setFeatured(featured: boolean): Promise<void> {
  if (!post.value) return
  try {
    const { data } = await forumApi.setFeatured(postId.value, featured)
    post.value = data
  } catch (err: unknown) {
    error.value = apiError(err, '操作失败')
  }
}

async function removePost(): Promise<void> {
  if (!confirm('删除该帖子？可在回收站恢复。') || !post.value) return
  await forumApi.remove(postId.value)
  router.push({ name: 'forum' })
}

onMounted(load)
</script>

<template>
  <div class="page" v-if="loading">加载中…</div>
  <div class="page" v-else-if="error">
    <p class="error">{{ error }}</p>
    <RouterLink to="/forum" class="btn">返回论坛</RouterLink>
  </div>

  <div class="page" v-else-if="post">
    <div class="page-head">
      <div class="title">
        <h1>{{ post.title }}</h1>
        <span class="sub">
          {{ boardLabel[post.board] }} · {{ post.author_username ?? '匿名' }} · {{ post.created_at }}
        </span>
      </div>
      <div class="page-head-actions">
        <button v-if="auth.isAuthenticated" class="btn" :class="{ 'btn-primary': liked }" @click="toggleLike">
          ♥ {{ post.likes_count }}
        </button>
        <button v-if="auth.isStaff" class="btn" @click="setFeatured(!post.is_featured)">
          {{ post.is_featured ? '取消精选' : '设为精选' }}
        </button>
        <button v-if="auth.isStaff || auth.userId === post.author_id" class="btn btn-danger" @click="removePost">删除</button>
        <ReportButton target-kind="forum_post" :target-id="post.id" :author-id="post.author_id" />
      </div>
    </div>

    <MarkdownView :source="post.content" />

    <div class="section">
      <div class="section-title">回复（{{ comments.length }} / {{ commentsTotal }}）</div>
      <div class="list">
        <div v-for="c in comments" :key="c.id" class="list-row" style="cursor: default">
          <div class="row-main">
            <span class="row-title">
              {{ c.author_username ?? '匿名' }}
              <span class="row-sub" style="margin-left: 8px">{{ c.created_at }}</span>
            </span>
            <span class="row-sub">{{ c.content }}</span>
          </div>
          <button class="linklike" @click="toggleCommentLike(c)">♥ {{ c.likes_count }}</button>
          <ReportButton
            target-kind="forum_comment"
            :target-id="c.id"
            :author-id="c.author_id"
            style="margin-left: 8px"
          />
          <button
            v-if="auth.isStaff || auth.userId === c.author_id"
            class="linklike"
            style="margin-left: 8px"
            @click="removeComment(c.id)"
          >
            删除
          </button>
        </div>
        <p v-if="comments.length === 0" class="meta" style="margin: 8px 0">还没有回复。</p>
        <p v-if="moreCommentsError" class="error">{{ moreCommentsError }}</p>
        <button
          v-if="comments.length < commentsTotal"
          class="btn"
          style="margin-top: 8px"
          @click="loadMoreComments"
        >
          加载更多回复（已显示 {{ comments.length }} / {{ commentsTotal }}）
        </button>
      </div>

      <form v-if="auth.isAuthenticated" class="form-stack" style="margin-top: 12px" @submit.prevent="submitComment">
        <textarea v-model="newComment" rows="3" placeholder="写下你的回复" maxlength="50000"></textarea>
        <div class="row gap">
          <button type="submit" class="btn">{{ solving ? '验证中…' : '回复' }}</button>
        </div>
      </form>
      <p v-else class="meta"><RouterLink to="/login">登录</RouterLink>后才能回复。</p>
      <p v-if="commentError" class="error">{{ commentError }}</p>
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
.list-row:hover {
  background: none;
}
</style>
