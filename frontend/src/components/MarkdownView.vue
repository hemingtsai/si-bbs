<script setup lang="ts">
import { computed } from 'vue'

import { hardenLinks, renderMarkdown } from '../lib/markdown'

const props = defineProps<{ source: string | null | undefined }>()
const html = computed(() => hardenLinks(renderMarkdown(props.source)))
</script>

<template>
  <div class="markdown-body" v-html="html"></div>
</template>

<style scoped>
.markdown-body {
  line-height: 1.7;
}
.markdown-body :deep(pre) {
  padding: 1rem;
  border-radius: 6px;
  overflow-x: auto;
  background: #1e1e1e;
}
.markdown-body :deep(code) {
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
}
.markdown-body :deep(img) {
  max-width: 100%;
}
.markdown-body :deep(table) {
  border-collapse: collapse;
}
.markdown-body :deep(td),
.markdown-body :deep(th) {
  border: 1px solid var(--si-border);
  padding: 0.25rem 0.75rem;
}
</style>