<script setup lang="ts">
import { ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'

import { useAuthStore } from '../../stores/auth'

const auth = useAuthStore()
const router = useRouter()
const route = useRoute()

const username = ref('')
const password = ref('')
const error = ref('')
const busy = ref(false)

async function submit(): Promise<void> {
  error.value = ''
  busy.value = true
  try {
    await auth.login({ username: username.value.trim(), password: password.value })
    const target = typeof route.query.redirect === 'string' ? route.query.redirect : '/'
    router.push(target)
  } catch (err: unknown) {
    error.value =
      (err as { response?: { data?: { error?: string } } })?.response?.data?.error ??
      '登录失败'
  } finally {
    busy.value = false
  }
}
</script>

<template>
  <div class="page">
    <div class="page-head">
      <div class="title">
        <h1>登录</h1>
        <span class="sub">使用已有账号进入</span>
      </div>
    </div>

    <form class="form-stack" @submit.prevent="submit">
      <label class="field">
        <span class="field-label">用户名</span>
        <input v-model="username" type="text" required autocomplete="username" />
      </label>
      <label class="field">
        <span class="field-label">密码</span>
        <input v-model="password" type="password" required autocomplete="current-password" />
      </label>
      <p v-if="error" class="error">{{ error }}</p>
      <div class="row gap">
        <button class="btn btn-primary" type="submit" :disabled="busy">
          {{ busy ? '登录中…' : '登录' }}
        </button>
        <RouterLink to="/register" class="btn">注册新账号</RouterLink>
      </div>
    </form>
  </div>
</template>

<style scoped>
.form-stack {
  display: flex;
  flex-direction: column;
  gap: 12px;
  max-width: 22rem;
}
.field {
  display: flex;
  flex-direction: column;
  gap: var(--field-gap);
}
</style>
