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
    error.value = axiosError(err, '登录失败')
  } finally {
    busy.value = false
  }
}

function axiosError(err: unknown, fallback: string): string {
  const message = (err as { response?: { data?: { error?: string } } })?.response?.data?.error
  return message ?? fallback
}
</script>

<template>
  <section class="auth">
    <h1>登录</h1>
    <form class="auth__form" @submit.prevent="submit">
      <label>
        用户名
        <input v-model="username" type="text" required autocomplete="username" />
      </label>
      <label>
        密码
        <input v-model="password" type="password" required autocomplete="current-password" />
      </label>
      <p v-if="error" class="auth__error">{{ error }}</p>
      <button type="submit" :disabled="busy">{{ busy ? '登录中…' : '登录' }}</button>
    </form>
    <p>还没有账号？<RouterLink to="/register">注册</RouterLink></p>
  </section>
</template>

<style scoped>
.auth__form {
  display: flex;
  flex-direction: column;
  gap: 0.75rem;
  max-width: 22rem;
}
.auth__form label {
  display: flex;
  flex-direction: column;
  gap: 0.25rem;
}
.auth__error {
  color: #b00020;
}
</style>