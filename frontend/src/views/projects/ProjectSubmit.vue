<script setup lang="ts">
import { ref } from 'vue'
import { useRouter } from 'vue-router'

import { projectsApi } from '../../api'
import { apiError } from '../../lib/errors'
import { PROJECT_CATEGORIES } from '../../lib/categories'

const router = useRouter()

const githubUrl = ref('')
const category = ref('')
const description = ref('')
const error = ref('')
const busy = ref(false)

async function submit(): Promise<void> {
  error.value = ''
  busy.value = true
  try {
    const { data } = await projectsApi.submit({
      github_url: githubUrl.value.trim(),
      category: category.value.trim(),
      description: description.value.trim() || undefined,
    })
    router.push({ name: 'project-detail', params: { id: data.id } })
  } catch (err: unknown) {
    error.value = apiError(err, '提交失败，请检查 GitHub 链接是否可访问')
  } finally {
    busy.value = false
  }
}
</script>

<template>
  <div class="page">
    <div class="page-head">
      <div class="title">
        <h1>提交项目</h1>
        <span class="sub">分享你的 GitHub 项目给社区</span>
      </div>
    </div>

    <form class="form-stack" @submit.prevent="submit">
      <label class="field">
        <span class="field-label">GitHub 链接</span>
        <input v-model="githubUrl" type="url" required placeholder="https://github.com/owner/repo" />
      </label>
      <label class="field">
        <span class="field-label">分类</span>
        <select v-model="category" required>
          <option value="" disabled>请选择分类</option>
          <option v-for="c in PROJECT_CATEGORIES" :key="c" :value="c">{{ c }}</option>
        </select>
      </label>
      <label class="field">
        <span class="field-label">一句话描述（可选，缺省取 GitHub 简介）</span>
        <input v-model="description" type="text" maxlength="200" />
      </label>
      <p v-if="error" class="error">{{ error }}</p>
      <p class="field-hint">提交后进入待审核状态，审核通过后才会出现在公开列表。</p>
      <div class="row gap">
        <button type="submit" class="btn btn-primary" :disabled="busy">{{ busy ? '提交中…' : '提交' }}</button>
      </div>
    </form>
  </div>
</template>

<style scoped>
.form-stack {
  display: flex;
  flex-direction: column;
  gap: 12px;
  max-width: 32rem;
}
.field {
  display: flex;
  flex-direction: column;
  gap: var(--field-gap);
}
</style>
