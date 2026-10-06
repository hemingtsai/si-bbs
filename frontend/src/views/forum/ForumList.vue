<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { forumApi } from '../../api'
import type { BoardInfo, ForumBoard, ForumPost, ForumRule } from '../../api/types'
import MarkdownView from '../../components/MarkdownView.vue'

const boards = ref<BoardInfo[]>([])
const activeBoard = ref<ForumBoard | ''>('')
const posts = ref<ForumPost[]>([])
const total = ref(0)
const page = ref(1)
const perPage = 20
const q = ref('')
const loading = ref(true)
const rules = ref<ForumRule[]>([])
const showRules = ref(true)

const boardLabel: Record<string, string> = { '': '全部', models: '模型讨论', tools: '工具交流', life: '谈天说地' }
const boardOf = (p: ForumPost) => p.board as keyof typeof boardLabel

async function loadBoards(): Promise<void> {
  const { data } = await forumApi.boards()
  boards.value = data
}

async function loadPosts(): Promise<void> {
  loading.value = true
  try {
    const { data } = await forumApi.list({
      board: activeBoard.value || undefined,
      q: q.value || undefined,
      page: page.value,
      per_page: perPage,
    })
    posts.value = data.items
    total.value = data.total
  } finally {
    loading.value = false
  }
}

async function loadRules(): Promise<void> {
  if (!activeBoard.value) {
    const { data } = await forumApi.rules()
    rules.value = data.length > 1 ? data.slice(0, 1) : data
  } else {
    const { data } = await forumApi.rules(activeBoard.value)
    rules.value = data
  }
}

async function switchBoard(slug: ForumBoard | ''): Promise<void> {
  activeBoard.value = slug
  page.value = 1
  await Promise.all([loadPosts(), loadRules()])
}

function search(): void {
  page.value = 1
  loadPosts()
}

onMounted(async () => {
  await loadBoards()
  await Promise.all([loadPosts(), loadRules()])
})
</script>

<template>
  <div class="page">
    <div class="page-head">
      <div class="title">
        <h1>论坛</h1>
        <span class="sub">三个板块，发帖即看，无需审核</span>
      </div>
      <div class="page-head-actions">
        <RouterLink to="/forum/new" class="btn btn-primary">发帖</RouterLink>
      </div>
    </div>

    <div class="controls">
      <button class="btn" :class="{ 'btn-primary': activeBoard === '' }" @click="switchBoard('')">全部</button>
      <button v-for="b in boards" :key="b.slug" class="btn" :class="{ 'btn-primary': activeBoard === b.slug }" @click="switchBoard(b.slug as ForumBoard)">
        {{ boardLabel[b.slug] }}（{{ b.post_count }}）
      </button>
      <form class="field" style="flex: 1; min-width: 160px" @submit.prevent="search">
        <input v-model="q" type="search" placeholder="搜索标题或正文" />
      </form>
    </div>

    <div v-if="rules.length" class="section">
      <div class="section-title">
        总站规与板规
        <button class="linklike" style="margin-left: 8px" @click="showRules = !showRules">
          {{ showRules ? '收起' : '展开' }}
        </button>
      </div>
      <div v-if="showRules">
        <div v-for="r in rules" :key="r.board" class="rule-block">
          <div class="section-hint">{{ boardLabel[r.board] ?? r.board }} · {{ r.title }}</div>
          <MarkdownView :source="r.content" />
        </div>
      </div>
    </div>

    <p v-if="loading" class="meta">加载中…</p>
    <p v-else-if="posts.length === 0" class="meta" style="margin-top: 16px">暂无帖子，来发第一篇。</p>

    <div v-else class="list" style="margin-top: 16px">
      <RouterLink v-for="post in posts" :key="post.id" class="list-row" :to="{ name: 'forum-detail', params: { id: post.id } }">
        <div class="row-main">
          <span class="row-title">
            <span v-if="post.is_featured" class="tag" style="border-color: var(--warn); color: var(--warn)">精选</span>
            {{ post.title }}
          </span>
          <span class="row-sub">
            {{ boardLabel[boardOf(post)] }} · {{ post.author_username ?? '匿名' }} · {{ post.created_at }}
          </span>
        </div>
        <span class="col-num">♥ {{ post.likes_count }}</span>
        <span class="col-num">↩ {{ post.comments_count }}</span>
      </RouterLink>
    </div>

    <nav v-if="total > perPage" class="pager">
      <button class="btn" :disabled="page <= 1" @click="page--; loadPosts()">上一页</button>
      <span class="mono">{{ page }} / {{ Math.ceil(total / perPage) }}</span>
      <button class="btn" :disabled="page * perPage >= total" @click="page++; loadPosts()">下一页</button>
    </nav>
  </div>
</template>

<style scoped>
.rule-block {
  border-top: 1px solid var(--line-strong);
  padding: 8px 0;
}
.tag {
  display: inline-block;
  padding: 0 6px;
  border: 1px solid var(--line-strong);
  font-size: 11px;
  color: var(--text-2);
  line-height: 18px;
  margin-right: 6px;
}
</style>
