<script setup lang="ts">
import { ref } from 'vue'

import { attachmentApi, type Attachment } from '../api'
import { apiError } from '../lib/errors'

const props = defineProps<{
  /// Hidden input id, so a label can act as the picker without extra JS.
  inputId?: string
}>()

const emit = defineEmits<{ insert: [markdown: string] }>()

const uploaded = ref<Attachment | null>(null)
const error = ref('')
const busy = ref(false)

/// The same allowlist as the server; the browser-side check only saves a round trip,
/// the server decides (it sniffs the bytes).
const ACCEPT = 'image/png,image/jpeg,image/gif,image/webp,application/pdf,text/plain,text/markdown'

async function pick(event: Event): Promise<void> {
  const input = event.target as HTMLInputElement
  const file = input.files?.[0]
  if (!file) return
  error.value = ''
  uploaded.value = null
  busy.value = true
  try {
    const { data } = await attachmentApi.upload(file)
    uploaded.value = data
  } catch (err) {
    error.value = apiError(err, '上传失败')
  } finally {
    busy.value = false
    // Allow re-picking the same file.
    input.value = ''
  }
}
</script>

<template>
  <div class="upload">
    <label class="field-label" :for="props.inputId ?? 'attachment-input'">附件</label>
    <input
      :id="props.inputId ?? 'attachment-input'"
      type="file"
      :accept="ACCEPT"
      :disabled="busy"
      @change="pick"
    />
    <span class="hint">图片 / PDF / 文本，最大 5MB。图片会直接显示在正文里。</span>
    <p v-if="busy" class="hint">上传中…</p>
    <p v-if="error" class="error">{{ error }}</p>
    <div v-if="uploaded" class="uploaded">
      <img
        v-if="uploaded.content_type.startsWith('image/')"
        class="preview"
        :src="uploaded.url"
        :alt="uploaded.filename"
      />
      <span class="mono">{{ uploaded.filename }}</span>
      <button type="button" class="linklike" @click="emit('insert', uploaded.markdown)">
        插入正文
      </button>
      <span class="hint">
        {{ Math.round(uploaded.size_bytes / 1024) }} KB · 也可以直接复制
        <code>{{ uploaded.url }}</code>
      </span>
    </div>
  </div>
</template>

<style scoped>
.upload {
  display: flex;
  flex-direction: column;
  gap: 4px;
  margin-bottom: 10px;
}
.uploaded {
  display: flex;
  align-items: center;
  gap: 10px;
  border: 1px solid var(--border);
  border-radius: 6px;
  padding: 8px;
}
.preview {
  max-width: 120px;
  max-height: 80px;
  border-radius: 4px;
}
.hint {
  color: var(--text-muted);
  font-size: 13px;
}
.error {
  color: var(--danger, #d33);
  font-size: 13px;
}
</style>
