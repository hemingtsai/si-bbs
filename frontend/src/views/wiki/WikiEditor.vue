<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'

import { wikiApi } from '../../api'
import { apiError } from '../../lib/errors'

const route = useRoute()
const router = useRouter()

const isEdit = computed(() => route.name === 'wiki-edit')
const pageId = ref<number | null>(null)

const title = ref('')
const category = ref('')
const content = ref('')
const status = ref<'draft' | 'published'>('draft')
/// Set for existing pages. Non-ASCII titles produce a generated slug, so this is
/// how a Chinese page gets a readable URL — and the old one keeps working.
const slug = ref('')
/// The revision this edit started from; the server refuses the save if somebody
/// else has written in the meantime.
const baseRevision = ref<number | null>(null)
const comment = ref('')
const error = ref('')
const conflict = ref('')
const busy = ref(false)
const loading = ref(isEdit.value)

async function loadForEdit(): Promise<void> {
  try {
    const { data } = await wikiApi.detail(String(route.params.slug))
    pageId.value = data.id
    title.value = data.title
    category.value = data.category
    content.value = data.content
    status.value = data.status
    slug.value = data.slug
    baseRevision.value = data.revision
  } catch {
    error.value = '无法加载页面'
  } finally {
    loading.value = false
  }
}

async function submit(): Promise<void> {
  error.value = ''
  if (!title.value.trim() || !category.value.trim() || !content.value.trim()) {
    error.value = '标题、分类和正文都不能为空'
    return
  }
  busy.value = true
  try {
    if (isEdit.value && pageId.value !== null) {
      await wikiApi.update(pageId.value, {
        title: title.value.trim(),
        category: category.value.trim(),
        content: content.value,
        status: status.value,
        slug: slug.value.trim() || undefined,
        base_revision: baseRevision.value ?? undefined,
        comment: comment.value.trim() || undefined,
      })
      router.push({ name: 'wiki-detail', params: { slug: slug.value.trim() } })
    } else {
      const { data } = await wikiApi.create({
        title: title.value.trim(),
        category: category.value.trim(),
        content: content.value,
        status: status.value,
        slug: slug.value.trim() || undefined,
        comment: comment.value.trim() || undefined,
      })
      router.push({ name: 'wiki-detail', params: { slug: data.slug } })
    }
  } catch (err: unknown) {
    if (isConflict(err)) {
      // Losing an edit silently is exactly what this guards against: say so plainly
      // and leave the text in the box.
      conflict.value = '有人在你编辑期间保存了这个页面。请先看历史对比，再决定是否覆盖。'
    } else {
      error.value = apiError(err, '保存失败')
    }
  } finally {
    busy.value = false
  }
}

function isConflict(err: unknown): boolean {
  return (
    typeof err === 'object' &&
    err !== null &&
    'response' in err &&
    (err as { response?: { status?: number } }).response?.status === 409
  )
}

onMounted(() => {
  if (isEdit.value) loadForEdit()
})
</script>

<template>
  <div class="page">
    <div class="page-head">
      <div class="title">
        <h1>{{ isEdit ? '编辑页面' : '新建页面' }}</h1>
      </div>
    </div>

    <p v-if="loading" class="meta">加载中…</p>
    <form v-else class="form-stack" @submit.prevent="submit">
      <label class="field">
        <span class="field-label">标题</span>
        <input v-model="title" type="text" required maxlength="200" />
      </label>
      <label class="field">
        <span class="field-label">分类</span>
        <input v-model="category" type="text" required maxlength="40" />
      </label>
      <label class="field">
        <span class="field-label">状态</span>
        <select v-model="status">
          <option value="draft">草稿</option>
          <option value="published">发布</option>
        </select>
      </label>
      <label class="field">
        <span class="field-label">链接别名（slug）</span>
        <input v-model="slug" type="text" maxlength="80" pattern="[a-z0-9-]*" placeholder="留空则由标题生成" />
        <span class="field-hint">
          只允许小写字母、数字与连字符。改动后旧链接仍然可用（会自动跳到新地址）。
        </span>
      </label>
      <label class="field">
        <span class="field-label">修改说明（可选）</span>
        <input v-model="comment" type="text" maxlength="200" placeholder="例如：修正错别字" />
      </label>
      <label class="field">
        <span class="field-label">正文（Markdown）</span>
        <textarea v-model="content" rows="18" required maxlength="200000" class="mono"></textarea>
      </label>
      <p v-if="error" class="error">{{ error }}</p>
      <p v-if="conflict" class="error">
        {{ conflict }}
        <RouterLink
          v-if="isEdit"
          class="linklike"
          :to="{ name: 'wiki-history', params: { slug: String(route.params.slug) } }"
        >
          打开历史
        </RouterLink>
      </p>
      <div class="row gap">
        <button type="submit" class="btn btn-primary" :disabled="busy">{{ busy ? '保存中…' : '保存' }}</button>
        <button type="button" class="btn" @click="router.back()">取消</button>
      </div>
    </form>
  </div>
</template>

<style scoped>
.form-stack {
  display: flex;
  flex-direction: column;
  gap: 12px;
  max-width: 46rem;
}
.field {
  display: flex;
  flex-direction: column;
  gap: var(--field-gap);
}
</style>
