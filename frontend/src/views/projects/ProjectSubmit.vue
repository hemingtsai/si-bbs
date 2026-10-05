<script setup lang="ts">
import { ref } from 'vue'
import { useRouter } from 'vue-router'

import { projectsApi } from '../../api'
import { apiError } from '../../lib/errors'

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
  <section>
    <h1>提交项目</h1>
    <form class="form" @submit.prevent="submit">
      <label>
        GitHub 链接
        <input v-model="githubUrl" type="url" required placeholder="https://github.com/owner/repo" />
      </label>
      <label>
        分类
        <input v-model="category" type="text" required placeholder="例如 dev-tools" />
      </label>
      <label>
        一句话描述（可选，缺省取 GitHub 简介）
        <input v-model="description" type="text" maxlength="200" />
      </label>
      <p v-if="error" class="error">{{ error }}</p>
      <p class="hint">提交后进入待审核状态，审核通过后才会出现在公开列表。</p>
      <button type="submit" :disabled="busy">{{ busy ? '提交中…' : '提交' }}</button>
    </form>
  </section>
</template>

<style scoped>
.form {
  display: flex;
  flex-direction: column;
  gap: 0.75rem;
  max-width: 32rem;
}
.form label {
  display: flex;
  flex-direction: column;
  gap: 0.25rem;
}
.error {
  color: #b00020;
}
.hint {
  color: var(--si-muted, #666);
  font-size: 0.9rem;
}
</style>
