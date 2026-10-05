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
  <div class="page">
    <div class="page-head">
      <div class="title">
        <h1>注册</h1>
        <span class="sub">创建一个新账号</span>
      </div>
    </div>

    <form class="form-stack" @submit.prevent="submit">
      <label class="field">
        <span class="field-label">用户名</span>
        <input v-model="username" type="text" required autocomplete="username" />
      </label>
      <label class="field">
        <span class="field-label">邮箱</span>
        <input v-model="email" type="email" required autocomplete="email" />
      </label>
      <label class="field">
        <span class="field-label">密码</span>
        <input v-model="password" type="password" required minlength="6" autocomplete="new-password" />
      </label>
      <label class="field">
        <span class="field-label">确认密码</span>
        <input v-model="confirm" type="password" required autocomplete="new-password" />
      </label>
      <p v-if="error" class="error">{{ error }}</p>
      <p v-if="notice" class="notice">{{ notice }}</p>
      <div class="row gap">
        <button class="btn btn-primary" type="submit" :disabled="busy">
          {{ busy ? '注册中…' : '注册' }}
        </button>
        <RouterLink to="/login" class="btn">去登录</RouterLink>
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
