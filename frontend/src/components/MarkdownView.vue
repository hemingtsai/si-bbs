<script setup lang="ts">
import { computed } from 'vue'

import { renderMarkdown } from '../lib/markdown'

const props = defineProps<{ source: string | null | undefined }>()
const html = computed(() => renderMarkdown(props.source))
</script>

<template>
  <div class="markdown-body" v-html="html"></div>
</template>
