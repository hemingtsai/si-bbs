<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'

import { authApi, reportApi } from '../api'
import { useAuthStore } from '../stores/auth'
import type { Me, MyReport } from '../api/types'

const auth = useAuthStore()

const profile = ref<Me | null>(null)
const displayName = ref('')
const bio = ref('')
const avatarUrl = ref('')
const profileMessage = ref('')
const profileError = ref('')
const savingProfile = ref(false)

const currentPassword = ref('')
const newPassword = ref('')
const confirmPassword = ref('')
const passwordMessage = ref('')
const passwordError = ref('')
const savingPassword = ref(false)

const emailPassword = ref('')
const newEmail = ref('')
const emailMessage = ref('')
const emailError = ref('')
const savingEmail = ref(false)

const reports = ref<MyReport[]>([])
const reportsError = ref('')

const avatarPreview = computed(() => avatarUrl.value.trim())

const statusLabel: Record<MyReport['status'], string> = {
  open: '待处理',
  resolved: '已处理',
  dismissed: '已驳回',
}

const kindLabel: Record<MyReport['target_kind'], string> = {
  forum_post: '论坛帖子',
  forum_comment: '论坛回复',
  wiki: 'Wiki 页面',
  project: '项目',
  comment: '项目评论',
}

onMounted(async () => {
  try {
    const { data } = await authApi.profile()
    profile.value = data
    displayName.value = data.display_name ?? ''
    bio.value = data.bio ?? ''
    avatarUrl.value = data.avatar_url ?? ''
    auth.persist({
      role: data.role,
      username: data.username,
      display_name: data.display_name,
      avatar_url: data.avatar_url,
    })
  } catch {
    profileError.value = '无法读取个人资料'
  }
  try {
    const { data } = await reportApi.mine()
    reports.value = data.items
  } catch {
    reportsError.value = '无法读取我的举报'
  }
})

async function saveProfile(): Promise<void> {
  profileMessage.value = ''
  profileError.value = ''
  savingProfile.value = true
  try {
    const data = await auth.updateProfile({
      display_name: displayName.value,
      bio: bio.value,
      avatar_url: avatarUrl.value,
    })
    profile.value = data
    displayName.value = data.display_name ?? ''
    bio.value = data.bio ?? ''
    avatarUrl.value = data.avatar_url ?? ''
    profileMessage.value = '已保存'
  } catch {
    profileError.value = '保存失败：昵称不超过 32 字、简介不超过 500 字，头像必须是 http(s) 地址'
  } finally {
    savingProfile.value = false
  }
}

async function savePassword(): Promise<void> {
  passwordMessage.value = ''
  passwordError.value = ''
  if (newPassword.value !== confirmPassword.value) {
    passwordError.value = '两次输入的新密码不一致'
    return
  }
  savingPassword.value = true
  try {
    // The store also swaps in the new token pair: every older token dies with the
    // password change.
    await auth.changePassword({
      current_password: currentPassword.value,
      new_password: newPassword.value,
    })
    currentPassword.value = ''
    newPassword.value = ''
    confirmPassword.value = ''
    passwordMessage.value = '密码已更新，其它设备上的登录已失效'
  } catch (err) {
    passwordError.value = isUnauthorized(err) ? '当前密码不正确' : '修改失败：新密码需 6–128 字符且不同于原密码'
  } finally {
    savingPassword.value = false
  }
}

async function saveEmail(): Promise<void> {
  emailMessage.value = ''
  emailError.value = ''
  savingEmail.value = true
  try {
    const { data } = await authApi.changeEmail({
      password: emailPassword.value,
      new_email: newEmail.value,
    })
    if (profile.value) {
      profile.value.email = data.email
    }
    emailPassword.value = ''
    newEmail.value = ''
    emailMessage.value = '邮箱已更新'
  } catch (err) {
    emailError.value = isUnauthorized(err) ? '密码不正确' : '修改失败：邮箱格式不对或已被占用'
  } finally {
    savingEmail.value = false
  }
}

function isUnauthorized(err: unknown): boolean {
  return (
    typeof err === 'object' &&
    err !== null &&
    'response' in err &&
    (err as { response?: { status?: number } }).response?.status === 401
  )
}
</script>

<template>
  <section class="page">
    <h1>个人设置</h1>
    <p class="sub">
      登录名 <span class="mono">{{ profile?.username ?? auth.username }}</span>
      <template v-if="profile"> · 注册于 {{ profile.created_at }}</template>
    </p>

    <form class="card" @submit.prevent="saveProfile">
      <h2>资料</h2>
      <label class="field">
        <span>昵称</span>
        <input v-model="displayName" maxlength="32" placeholder="留空则显示登录名" />
      </label>
      <label class="field">
        <span>简介</span>
        <textarea v-model="bio" rows="3" maxlength="500" placeholder="不超过 500 字，可换行" />
      </label>
      <label class="field">
        <span>头像地址</span>
        <input v-model="avatarUrl" maxlength="500" placeholder="https://…（仅 http/https）" />
      </label>
      <div class="row">
        <img v-if="avatarPreview" class="avatar" :src="avatarPreview" alt="头像预览" />
        <span class="hint">昵称会显示在论坛、Wiki 与项目评论上；管理端仍使用登录名。</span>
      </div>
      <p v-if="profileMessage" class="ok">{{ profileMessage }}</p>
      <p v-if="profileError" class="error">{{ profileError }}</p>
      <button class="btn" type="submit" :disabled="savingProfile">
        {{ savingProfile ? '保存中…' : '保存资料' }}
      </button>
    </form>

    <form class="card" @submit.prevent="savePassword">
      <h2>修改密码</h2>
      <p class="hint">修改后此前签发的所有 token 立即失效，包括其它设备上的登录。</p>
      <label class="field">
        <span>当前密码</span>
        <input v-model="currentPassword" type="password" autocomplete="current-password" />
      </label>
      <label class="field">
        <span>新密码</span>
        <input v-model="newPassword" type="password" autocomplete="new-password" />
      </label>
      <label class="field">
        <span>确认新密码</span>
        <input v-model="confirmPassword" type="password" autocomplete="new-password" />
      </label>
      <p v-if="passwordMessage" class="ok">{{ passwordMessage }}</p>
      <p v-if="passwordError" class="error">{{ passwordError }}</p>
      <button class="btn" type="submit" :disabled="savingPassword">
        {{ savingPassword ? '提交中…' : '修改密码' }}
      </button>
    </form>

    <form class="card" @submit.prevent="saveEmail">
      <h2>修改邮箱</h2>
      <label class="field">
        <span>当前邮箱</span>
        <input :value="profile?.email ?? ''" disabled />
      </label>
      <label class="field">
        <span>新邮箱</span>
        <input v-model="newEmail" type="email" />
      </label>
      <label class="field">
        <span>当前密码</span>
        <input v-model="emailPassword" type="password" autocomplete="current-password" />
      </label>
      <p v-if="emailMessage" class="ok">{{ emailMessage }}</p>
      <p v-if="emailError" class="error">{{ emailError }}</p>
      <button class="btn" type="submit" :disabled="savingEmail">
        {{ savingEmail ? '提交中…' : '修改邮箱' }}
      </button>
    </form>

    <section class="card">
      <h2>我的举报</h2>
      <p v-if="reportsError" class="error">{{ reportsError }}</p>
      <p v-else-if="reports.length === 0" class="hint">还没有提交过举报。</p>
      <ul v-else class="reports">
        <li v-for="report in reports" :key="report.id">
          <div class="report-head">
            <span class="tag" :class="report.status">{{ statusLabel[report.status] }}</span>
            <span class="hint">{{ kindLabel[report.target_kind] }} · {{ report.created_at }}</span>
          </div>
          <div class="row-title">{{ report.target_title ?? `#${report.target_id}` }}</div>
          <div class="hint">理由：{{ report.reason }}</div>
          <div v-if="report.note" class="hint">处理说明：{{ report.note }}</div>
        </li>
      </ul>
    </section>
  </section>
</template>

<style scoped>
.page {
  max-width: 720px;
}
.card {
  border: 1px solid var(--border);
  border-radius: 8px;
  padding: 16px;
  margin: 16px 0;
  background: var(--surface);
}
.card h2 {
  margin: 0 0 12px;
  font-size: 16px;
}
.field {
  display: flex;
  flex-direction: column;
  gap: 4px;
  margin-bottom: 10px;
}
.field span {
  font-size: 13px;
  color: var(--text-muted);
}
.field input,
.field textarea {
  border: 1px solid var(--border);
  border-radius: 6px;
  padding: 8px;
  background: var(--bg);
  color: inherit;
  font: inherit;
}
.field input:disabled {
  opacity: 0.6;
}
.row {
  display: flex;
  align-items: center;
  gap: 10px;
}
.avatar {
  width: 48px;
  height: 48px;
  border-radius: 50%;
  object-fit: cover;
  border: 1px solid var(--border);
}
.btn {
  border: 1px solid var(--border);
  border-radius: 6px;
  padding: 6px 14px;
  background: var(--bg);
  color: inherit;
  cursor: pointer;
}
.btn:disabled {
  opacity: 0.6;
  cursor: default;
}
.hint {
  color: var(--text-muted);
  font-size: 13px;
}
.ok {
  color: var(--accent);
  font-size: 13px;
}
.error {
  color: var(--danger, #d33);
  font-size: 13px;
}
.reports {
  list-style: none;
  padding: 0;
  margin: 0;
}
.reports li {
  border-top: 1px solid var(--border);
  padding: 10px 0;
}
.report-head {
  display: flex;
  align-items: center;
  gap: 8px;
}
.tag {
  border-radius: 999px;
  padding: 1px 8px;
  font-size: 12px;
  border: 1px solid var(--border);
}
.tag.open {
  border-color: var(--accent);
  color: var(--accent);
}
.row-title {
  font-weight: 600;
  margin: 4px 0;
}
</style>
