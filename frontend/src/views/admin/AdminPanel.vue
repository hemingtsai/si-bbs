<script setup lang="ts">
import { onMounted, ref } from 'vue'

import { adminApi } from '../../api'
import type { AdminUser, AuditEntry, Role, Stats } from '../../api/types'
import { apiError } from '../../lib/errors'

const users = ref<AdminUser[]>([])
const total = ref(0)
const page = ref(1)
const perPage = 10
const q = ref('')
const roleFilter = ref<Role | ''>('')
const stats = ref<Stats | null>(null)
const error = ref('')
const loading = ref(true)
const audit = ref<AuditEntry[]>([])
const auditTotal = ref(0)
const auditAction = ref('')
const auditError = ref('')

async function load(): Promise<void> {
  loading.value = true
  error.value = ''
  try {
    const [usersRes, statsRes] = await Promise.all([
      adminApi.users({ q: q.value || undefined, role: roleFilter.value || undefined, page: page.value, per_page: perPage }),
      adminApi.stats(),
    ])
    users.value = usersRes.data.items
    total.value = usersRes.data.total
    stats.value = statsRes.data
  } catch (err: unknown) {
    error.value = apiError(err, '加载失败')
  } finally {
    loading.value = false
  }
}

async function setRole(user: AdminUser, role: Role): Promise<void> {
  try {
    await adminApi.setRole(user.id, role)
    await load()
  } catch (err: unknown) {
    error.value = apiError(err, '修改角色失败')
  }
}

async function toggleBan(user: AdminUser): Promise<void> {
  try {
    await adminApi.setBan(user.id, user.banned === 0)
    await load()
  } catch (err: unknown) {
    error.value = apiError(err, '操作失败')
  }
}

function refine(): void {
  page.value = 1
  load()
}

/// The audit log answers "who changed this" after the fact, so it is read-only and
/// filterable by action.
async function loadAudit(): Promise<void> {
  auditError.value = ''
  try {
    const { data } = await adminApi.audit({
      action: auditAction.value || undefined,
      per_page: 50,
    })
    audit.value = data.items
    auditTotal.value = data.total
  } catch (err: unknown) {
    auditError.value = apiError(err, '无法加载审计日志')
  }
}

onMounted(async () => {
  await load()
  await loadAudit()
})
</script>

<template>
  <div class="page">
    <div class="page-head">
      <div class="title">
        <h1>管理后台</h1>
      </div>
    </div>

    <div v-if="stats" class="kv" style="max-width: 32rem; margin-bottom: 24px">
      <div class="kv-row"><span class="kv-key">用户</span><span class="kv-val">{{ stats.users }}（封禁 {{ stats.users_banned }}）</span></div>
      <div class="kv-row"><span class="kv-key">项目</span><span class="kv-val">{{ stats.projects_approved }} 已通过 / {{ stats.projects_pending }} 待审核</span></div>
      <div class="kv-row"><span class="kv-key">Wiki</span><span class="kv-val">{{ stats.wiki_published }} 已发布</span></div>
      <div class="kv-row"><span class="kv-key">评论</span><span class="kv-val">{{ stats.comments }}</span></div>
      <div class="kv-row"><span class="kv-key">评分</span><span class="kv-val">{{ stats.ratings }}</span></div>
      <div class="kv-row"><span class="kv-key">回收站</span><span class="kv-val">{{ stats.trashed }}</span></div>
    </div>

    <form class="controls" @submit.prevent="refine">
      <label class="field">
        <span class="field-label">搜索</span>
        <input v-model="q" type="search" placeholder="用户名或邮箱" />
      </label>
      <label class="field">
        <span class="field-label">角色</span>
        <select v-model="roleFilter" @change="refine">
          <option value="">全部</option>
          <option value="admin">admin</option>
          <option value="moderator">moderator</option>
          <option value="user">user</option>
        </select>
      </label>
      <div class="field field-actions">
        <button type="submit" class="btn">搜索</button>
      </div>
    </form>

    <p v-if="loading" class="meta">加载中…</p>
    <p v-else-if="error" class="error">{{ error }}</p>

    <table v-else>
      <thead>
        <tr>
          <th>用户名</th>
          <th>邮箱</th>
          <th>角色</th>
          <th>状态</th>
          <th>注册时间</th>
          <th></th>
        </tr>
      </thead>
      <tbody>
        <tr v-for="user in users" :key="user.id">
          <td>{{ user.username }}</td>
          <td>{{ user.email }}</td>
          <td>
            <select
              :value="user.role"
              @change="setRole(user, ($event.target as HTMLSelectElement).value as Role)"
            >
              <option value="admin">admin</option>
              <option value="moderator">moderator</option>
              <option value="user">user</option>
            </select>
          </td>
          <td>
            <span v-if="user.banned" class="status status-rejected">已封禁</span>
            <span v-else class="dot-label"><span class="dot dot-ok"></span>正常</span>
          </td>
          <td class="mono">{{ user.created_at }}</td>
          <td>
            <button class="btn" @click="toggleBan(user)">{{ user.banned ? '解封' : '封禁' }}</button>
          </td>
        </tr>
      </tbody>
    </table>

    <section class="section">
      <div class="section-title">审计日志（{{ auditTotal }}）</div>
      <div class="controls">
        <label class="field">
          <span class="field-label">动作</span>
          <select v-model="auditAction" @change="loadAudit">
            <option value="">全部</option>
            <option value="role.change">改角色</option>
            <option value="user.ban">封禁</option>
            <option value="user.unban">解封</option>
            <option value="project.review">项目审核</option>
            <option value="forum.feature">论坛精选</option>
            <option value="forum.rule_update">板块规则</option>
            <option value="trash.purge">彻底删除</option>
            <option value="trash.restore">回收站恢复</option>
            <option value="report.resolve">举报处理</option>
          </select>
        </label>
      </div>
      <p v-if="auditError" class="error">{{ auditError }}</p>
      <p v-else-if="audit.length === 0" class="meta">没有匹配的审计记录。</p>
      <table v-else>
        <thead>
          <tr>
            <th>时间</th>
            <th>操作者</th>
            <th>动作</th>
            <th>对象</th>
            <th>详情</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="entry in audit" :key="entry.id">
            <td class="mono">{{ entry.created_at }}</td>
            <td>{{ entry.actor_username ?? entry.actor_id }}</td>
            <td class="mono">{{ entry.action }}</td>
            <td>{{ entry.target_kind }}<template v-if="entry.target_id"> #{{ entry.target_id }}</template></td>
            <td>{{ entry.detail ?? '—' }}</td>
          </tr>
        </tbody>
      </table>
    </section>

    <nav v-if="total > perPage" class="pager">
      <button class="btn" :disabled="page <= 1" @click="page--; load()">上一页</button>
      <span class="mono">{{ page }} / {{ Math.ceil(total / perPage) }}</span>
      <button class="btn" :disabled="page * perPage >= total" @click="page++; load()">下一页</button>
    </nav>
  </div>
</template>

<style scoped>
.field-actions {
  justify-content: flex-end;
  flex-direction: row;
  align-items: flex-end;
}
</style>
