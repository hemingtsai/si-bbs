<script setup lang="ts">
import { ref } from 'vue'
import { useRouter } from 'vue-router'

import { useAuthStore } from '../../stores/auth'

const auth = useAuthStore()
const router = useRouter()

const username = ref('')
const email = ref('')
const password = ref('')
const confirm = ref('')
const error = ref('')
const notice = ref('')
const busy = ref(false)

async function submit(): Promise<void> {
  error.value = ''
  notice.value = ''

  if (password.value.length < 6) {
    error.value = '密码至少 6 位'
    return
  }
  if (password.value !== confirm.value) {
    error.value = '两次输入的密码不一致'
    return
  }

  busy.value = true
  try {
    await auth.register({
      username: username.value.trim(),
      email: email.value.trim(),
      password: password.value,
    })
    notice.value = '注册成功，请登录。'
    setTimeout(() => router.push({ name: 'login' }), 800)
  } catch (err: unknown) {
    error.value =
      (err as { response?: { data?: { error?: string } } })?.response?.data?.error ??
      '注册失败，请重试'
  } finally {
    busy.value = false
  }
}
</script>

<template>
  <section class="auth">
    <h1>注册</h1>
    <form class="auth__form" @submit.prevent="submit">
      <label>
        用户名
        <input v-model="username" type="text" required autocomplete="username" />
      </label>
      <label>
        邮箱
        <input v-model="email" type="email" required autocomplete="email" />
      </label>
      <label>
        密码
        <input
          v-model="password"
          type="password"
          required
          minlength="6"
          autocomplete="new-password"
        />
      </label>
      <label>
        确认密码
        <input
          v-model="confirm"
          type="password"
          required
          autocomplete="new-password"
        />
      </label>
      <p v-if="error" class="auth__error">{{ error }}</p>
      <p v-if="notice" class="auth__notice">{{ notice }}</p>
      <button type="submit" :disabled="busy">{{ busy ? '注册中…' : '注册' }}</button>
    </form>
    <p>已有账号？<RouterLink to="/login">登录</RouterLink></p>
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
.auth__notice {
  color: #2e7d32;
}
</style>