<script setup lang="ts">
import { ref } from 'vue'

import { reportApi } from '../api'
import { useAuthStore } from '../stores/auth'

const props = defineProps<{
  targetKind: 'forum_post' | 'forum_comment' | 'wiki' | 'project' | 'comment'
  targetId: number
  /// Content author, so we can avoid offering "report" on one's own writing.
  authorId?: number
}>()

const auth = useAuthStore()
const open = ref(false)
const reason = ref('')
const message = ref('')
const error = ref('')
const busy = ref(false)

const hidden = () =>
  !auth.isAuthenticated || (props.authorId !== undefined && props.authorId === auth.userId)

async function submit(): Promise<void> {
  message.value = ''
  error.value = ''
  if (!reason.value.trim()) {
    error.value = '请填写举报理由'
    return
  }
  busy.value = true
  try {
    const { data } = await reportApi.create({
      target_kind: props.targetKind,
      target_id: props.targetId,
      reason: reason.value.trim(),
    })
    // The API answers "already reported" with 200 instead of an error: the user's
    // intent is satisfied either way, so say so rather than showing a failure.
    message.value =
      data.status === 'already reported' ? '你已经举报过这条内容' : '已提交，版主会尽快处理'
    reason.value = ''
    open.value = false
  } catch {
    error.value = '提交失败，请稍后再试'
  } finally {
    busy.value = false
  }
}
</script>

<template>
  <span v-if="!hidden()" class="report">
    <button v-if="!open" class="linklike" @click="open = true">举报</button>
    <span v-else class="form">
      <input
        v-model="reason"
        type="text"
        maxlength="500"
        placeholder="举报理由（例如：广告、与主题无关）"
        @keyup.enter="submit"
      />
      <button class="linklike" :disabled="busy" @click="submit">
        {{ busy ? '提交中…' : '提交' }}
      </button>
      <button class="linklike" @click="open = false">取消</button>
    </span>
    <span v-if="message" class="ok">{{ message }}</span>
    <span v-if="error" class="error">{{ error }}</span>
  </span>
</template>

<style scoped>
.report {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  font-size: 13px;
}
.form {
  display: inline-flex;
  align-items: center;
  gap: 6px;
}
.form input {
  border: 1px solid var(--border);
  border-radius: 6px;
  padding: 3px 8px;
  background: var(--bg);
  color: inherit;
  font: inherit;
  min-width: 240px;
}
.ok {
  color: var(--accent);
}
.error {
  color: var(--danger, #d33);
}
</style>
