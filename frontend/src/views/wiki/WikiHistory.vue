<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'

import { wikiApi } from '../../api'
import { apiError } from '../../lib/errors'
import { useAuthStore } from '../../stores/auth'
import type { WikiDiff, WikiPage, WikiRevision } from '../../api/types'

const route = useRoute()
const router = useRouter()
const auth = useAuthStore()

const page = ref<WikiPage | null>(null)
const revisions = ref<WikiRevision[]>([])
const total = ref(0)
const diff = ref<WikiDiff | null>(null)
const error = ref('')
const loading = ref(true)
const notice = ref('')

const canEdit = computed(
  () => auth.isStaff || (page.value !== null && auth.userId === page.value.author_id),
)

/// `?from=2&to=3` drives the comparison; without it the page is just the list.
const from = computed(() => numberOrNull(route.query.from))
const to = computed(() => numberOrNull(route.query.to))

function numberOrNull(raw: unknown): number | null {
  const value = Number(Array.isArray(raw) ? raw[0] : raw)
  return Number.isInteger(value) && value > 0 ? value : null
}

async function load(): Promise<void> {
  loading.value = true
  error.value = ''
  try {
    const { data } = await wikiApi.detail(String(route.params.slug))
    page.value = data
    const history = await wikiApi.revisions(data.id)
    revisions.value = history.data.items
    total.value = history.data.total
  } catch {
    error.value = '无法加载页面历史'
    loading.value = false
    return
  }

  diff.value = null
  if (from.value !== null && to.value !== null) {
    try {
      const { data } = await wikiApi.diff(page.value.id, from.value, to.value)
      diff.value = data
    } catch {
      error.value = '无法对比这两个版本'
    }
  }
  loading.value = false
}

onMounted(load)
// A renamed page answers on its old slug too; follow it to the canonical URL so the
// comparison links keep pointing at one place.
watch(
  () => page.value?.slug,
  (slug) => {
    if (slug && slug !== route.params.slug) {
      router.replace({ name: 'wiki-history', params: { slug }, query: route.query })
    }
  },
)
watch(() => route.query, load)

function compare(current: number, previous: number): void {
  router.push({
    name: 'wiki-history',
    params: { slug: String(route.params.slug) },
    query: { from: String(previous), to: String(current) },
  })
}

async function revert(no: number): Promise<void> {
  if (!page.value) return
  if (!window.confirm(`把第 ${no} 版恢复为一个新版本？历史不会被改写，之后还可以再回滚。`)) return
  notice.value = ''
  error.value = ''
  try {
    const { data } = await wikiApi.revert(page.value.id, no, {
      base_revision: page.value.revision,
    })
    page.value = data
    notice.value = `已恢复为第 ${no} 版的内容（当前为第 ${data.revision} 版）`
    await load()
  } catch (err) {
    error.value = apiError(err, '回滚失败')
  }
}
</script>

<template>
  <section class="page">
    <div class="page-head">
      <div class="title">
        <h1>历史：{{ page?.title ?? '' }}</h1>
        <span class="meta">共 {{ total }} 个版本 · 当前第 {{ page?.revision ?? '?' }} 版</span>
      </div>
      <div class="row gap">
        <RouterLink
          v-if="page"
          class="btn"
          :to="{ name: 'wiki-detail', params: { slug: page.slug } }"
        >
          返回页面
        </RouterLink>
        <RouterLink
          v-if="canEdit && page"
          class="btn"
          :to="{ name: 'wiki-edit', params: { slug: page.slug } }"
        >
          编辑
        </RouterLink>
      </div>
    </div>

    <p v-if="loading" class="meta">加载中…</p>
    <p v-else-if="error" class="error">{{ error }}</p>
    <p v-if="notice" class="ok">{{ notice }}</p>

    <template v-if="!loading">
      <div v-if="diff" class="card">
        <div class="diff-head">
          <h2>第 {{ diff.from }} 版 → 第 {{ diff.to }} 版</h2>
          <span class="meta">+{{ diff.added }} / -{{ diff.removed }}</span>
          <RouterLink
            class="linklike"
            :to="{ name: 'wiki-history', params: { slug: String(route.params.slug) } }"
          >
            关闭对比
          </RouterLink>
        </div>
        <p v-if="diff.coarse" class="meta">
          改动过大，已按"整段替换"呈现（服务端有意的降级，避免逐行对比把请求拖死）。
        </p>
        <pre class="diff"><code><span
            v-for="(line, index) in diff.lines"
            :key="index"
            class="line"
            :class="line.kind"
          >{{ line.kind === 'add' ? '+' : line.kind === 'remove' ? '-' : ' ' }} {{ line.text }}
</span></code></pre>
      </div>

      <ul class="revisions">
        <li v-for="revision in revisions" :key="revision.revision_no" class="revision">
          <div class="revision-main">
            <span class="mono">第 {{ revision.revision_no }} 版</span>
            <span class="meta">
              {{ revision.author_username ?? '匿名' }} · {{ revision.created_at }} ·
              {{ revision.content_chars }} 字符
            </span>
            <span v-if="revision.comment" class="meta">说明：{{ revision.comment }}</span>
            <span v-if="revision.revision_no === (page?.revision ?? 0)" class="tag">当前</span>
          </div>
          <div class="row gap">
            <button
              v-if="revision.revision_no > 1"
              class="linklike"
              @click="compare(revision.revision_no, revision.revision_no - 1)"
            >
              对比上一版
            </button>
            <button
              v-if="page && revision.revision_no !== page.revision"
              class="linklike"
              @click="compare(page.revision, revision.revision_no)"
            >
              与当前版对比
            </button>
            <button
              v-if="canEdit && page && revision.revision_no !== page.revision"
              class="linklike"
              @click="revert(revision.revision_no)"
            >
              回滚到此版
            </button>
          </div>
        </li>
      </ul>
    </template>
  </section>
</template>

<style scoped>
.card {
  border: 1px solid var(--border);
  border-radius: 8px;
  padding: 12px 16px;
  margin-bottom: 16px;
}
.diff-head {
  display: flex;
  align-items: baseline;
  gap: 12px;
}
.diff-head h2 {
  font-size: 15px;
  margin: 0;
}
.diff {
  overflow-x: auto;
  font-size: 13px;
  line-height: 1.5;
  margin: 8px 0 0;
}
.line {
  display: block;
  white-space: pre;
}
.line.add {
  background: rgba(46, 160, 67, 0.15);
}
.line.remove {
  background: rgba(248, 81, 73, 0.15);
}
.revisions {
  list-style: none;
  padding: 0;
  margin: 0;
}
.revision {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 12px;
  border-bottom: 1px solid var(--border);
  padding: 10px 0;
}
.revision-main {
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.tag {
  align-self: flex-start;
  border: 1px solid var(--accent);
  color: var(--accent);
  border-radius: 999px;
  padding: 0 8px;
  font-size: 12px;
}
.ok {
  color: var(--accent);
}
</style>
