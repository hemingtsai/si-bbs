<script setup lang="ts">
import { nextTick, ref } from 'vue'
import { useRouter } from 'vue-router'

import { forumApi } from '../../api'
import type { ForumBoard } from '../../api/types'
import { apiError } from '../../lib/errors'
import { withPow } from '../../lib/pow'
import { useSolving } from '../../lib/usePow'
import AttachmentUpload from '../../components/AttachmentUpload.vue'

const router = useRouter()

const board = ref<ForumBoard>('models')
const title = ref('')
const content = ref('')
const error = ref('')
const busy = ref(false)
const solving = useSolving()
const contentEl = ref<HTMLTextAreaElement | null>(null)

/// Insert an attachment at the caret instead of appending to the end.
function insertIntoContent(snippet: string): void {
  const el = contentEl.value
  if (!el) {
    content.value += `\n${snippet}\n`
    return
  }
  const start = el.selectionStart ?? content.value.length
  const end = el.selectionEnd ?? start
  content.value = content.value.slice(0, start) + snippet + content.value.slice(end)
  void nextTick(() => {
    el.focus()
    el.selectionStart = el.selectionEnd = start + snippet.length
  })
}

async function submit(): Promise<void> {
  error.value = ''
  if (!title.value.trim() || !content.value.trim()) {
    error.value = '标题和正文不能为空'
    return
  }
  busy.value = true
  try {
    const { data } = await withPow((pow) =>
      forumApi.create({
        board: board.value,
        title: title.value.trim(),
        content: content.value,
        pow,
      }),
    )
    router.push({ name: 'forum-detail', params: { id: data.id } })
  } catch (err: unknown) {
    error.value = apiError(err, '发帖失败')
  } finally {
    busy.value = false
  }
}
</script>

<template>
  <div class="page">
    <div class="page-head">
      <div class="title"><h1>发新帖</h1><span class="sub">发帖即看，无需审核</span></div>
    </div>

    <form class="form-stack" @submit.prevent="submit">
      <label class="field">
        <span class="field-label">板块</span>
        <select v-model="board">
          <option value="models">模型讨论</option>
          <option value="tools">工具交流</option>
          <option value="life">谈天说地</option>
        </select>
      </label>
      <label class="field">
        <span class="field-label">标题</span>
        <input v-model="title" type="text" required maxlength="200" placeholder="一句话标题" />
      </label>
      <label class="field">
        <span class="field-label">正文（Markdown）</span>
        <textarea ref="contentEl" v-model="content" rows="14" required maxlength="50000" class="mono"></textarea>
      </label>
      <AttachmentUpload input-id="forum-attachment" @insert="insertIntoContent" />
      <p v-if="error" class="error">{{ error }}</p>
      <div class="row gap">
        <button type="submit" class="btn btn-primary" :disabled="busy">{{ solving ? '验证中…' : busy ? '发送中…' : '发送' }}</button>
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
