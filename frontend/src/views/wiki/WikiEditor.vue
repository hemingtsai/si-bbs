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
const error = ref('')
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
      })
      router.back()
    } else {
      const { data } = await wikiApi.create({
        title: title.value.trim(),
        category: category.value.trim(),
        content: content.value,
        status: status.value,
      })
      router.push({ name: 'wiki-detail', params: { slug: data.slug } })
    }
  } catch (err: unknown) {
    error.value = apiError(err, '保存失败')
  } finally {
    busy.value = false
  }
}

onMounted(() => {
  if (isEdit.value) loadForEdit()
})
</script>

<template>
  <section>
    <h1>{{ isEdit ? '编辑页面' : '新建页面' }}</h1>
    <p v-if="loading">加载中…</p>
    <form v-else class="form" @submit.prevent="submit">
      <label>
        标题
        <input v-model="title" type="text" required maxlength="200" />
      </label>
      <label>
        分类
        <input v-model="category" type="text" required maxlength="40" />
      </label>
      <label>
        状态
        <select v-model="status">
          <option value="draft">草稿</option>
          <option value="published">发布</option>
        </select>
      </label>
      <label>
        正文（Markdown）
        <textarea v-model="content" rows="18" required maxlength="200000"></textarea>
      </label>
      <p v-if="error" class="error">{{ error }}</p>
      <button type="submit" :disabled="busy">{{ busy ? '保存中…' : '保存' }}</button>
    </form>
  </section>
</template>

<style scoped>
.form {
  display: flex;
  flex-direction: column;
  gap: 0.75rem;
  max-width: 46rem;
}
.form label {
  display: flex;
  flex-direction: column;
  gap: 0.25rem;
}
.error {
  color: #b00020;
}
textarea {
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
}
</style>