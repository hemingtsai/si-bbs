<script setup lang="ts">
import { onMounted, ref } from 'vue'

import { forumApi, projectsApi, wikiApi } from '../api'
import type { ForumPostSummary, ProjectSummary, WikiPageSummary } from '../api/types'

/// The home page used to be a static menu; it now summarises what actually changed,
/// which is the reason to open a community site at all.
const posts = ref<ForumPostSummary[]>([])
const projects = ref<ProjectSummary[]>([])
const pages = ref<WikiPageSummary[]>([])
const loading = ref(true)

const boardLabel: Record<string, string> = {
  models: '模型讨论',
  tools: '工具交流',
  life: '谈天说地',
}

onMounted(async () => {
  // Three independent lists; one failing endpoint must not blank the whole page.
  const [forum, repo, wiki] = await Promise.allSettled([
    forumApi.list({ per_page: 5 }),
    projectsApi.list({ per_page: 5, sort: 'recent' }),
    wikiApi.list({ per_page: 5 }),
  ])
  if (forum.status === 'fulfilled') posts.value = forum.value.data.items
  if (repo.status === 'fulfilled') projects.value = repo.value.data.items
  if (wiki.status === 'fulfilled') pages.value = wiki.value.data.items
  loading.value = false
})
</script>

<template>
  <div class="page">
    <div class="page-head">
      <div class="title">
        <h1>SI BBS</h1>
        <span class="sub">AI 主题社区：Wiki 知识库 + 软件发布及索引</span>
      </div>
      <div class="page-head-actions">
        <RouterLink class="btn" to="/projects/submit">提交项目</RouterLink>
        <RouterLink class="btn" to="/wiki/new">新建 Wiki</RouterLink>
      </div>
    </div>

    <p v-if="loading" class="meta">加载中…</p>

    <template v-else>
      <section class="section">
        <div class="section-head">
          <div class="section-title">最新帖子</div>
          <RouterLink class="linklike" to="/forum">全部</RouterLink>
        </div>
        <p v-if="posts.length === 0" class="meta">还没有帖子。</p>
        <div v-else class="list">
          <RouterLink
            v-for="post in posts"
            :key="post.id"
            class="list-row"
            :to="{ name: 'forum-detail', params: { id: post.id } }"
          >
            <div class="row-main">
              <span class="row-title">{{ post.title }}</span>
              <span class="row-sub">
                {{ boardLabel[post.board] ?? post.board }} · {{ post.author_username ?? '匿名' }} ·
                {{ post.created_at }}
              </span>
            </div>
            <span class="row-num mono">♥ {{ post.likes_count }} · ↩ {{ post.comments_count }}</span>
          </RouterLink>
        </div>
      </section>

      <section class="section">
        <div class="section-head">
          <div class="section-title">新收录项目</div>
          <RouterLink class="linklike" to="/projects">全部</RouterLink>
        </div>
        <p v-if="projects.length === 0" class="meta">还没有收录项目。</p>
        <div v-else class="list">
          <RouterLink
            v-for="project in projects"
            :key="project.id"
            class="list-row"
            :to="{ name: 'project-detail', params: { id: project.id } }"
          >
            <div class="row-main">
              <span class="row-title">{{ project.name }}</span>
              <span class="row-sub">
                {{ project.owner }}/{{ project.repo }}
                <template v-if="project.description"> · {{ project.description }}</template>
              </span>
            </div>
            <span class="row-num mono">★ {{ project.stars }}</span>
          </RouterLink>
        </div>
      </section>

      <section class="section">
        <div class="section-head">
          <div class="section-title">最近 Wiki 变更</div>
          <RouterLink class="linklike" to="/wiki">全部</RouterLink>
        </div>
        <p v-if="pages.length === 0" class="meta">还没有 Wiki 页面。</p>
        <div v-else class="list">
          <RouterLink
            v-for="page in pages"
            :key="page.id"
            class="list-row"
            :to="{ name: 'wiki-detail', params: { slug: page.slug } }"
          >
            <div class="row-main">
              <span class="row-title">{{ page.title }}</span>
              <span class="row-sub">
                {{ page.category }} · {{ page.author_username ?? '未知作者' }} ·
                {{ page.updated_at }}
              </span>
            </div>
            <span class="row-num mono">第 {{ page.revision }} 版</span>
          </RouterLink>
        </div>
      </section>
    </template>
  </div>
</template>

<style scoped>
.section-head {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
}
.row-num {
  color: var(--text-2);
  font-size: 13px;
  white-space: nowrap;
}
</style>
