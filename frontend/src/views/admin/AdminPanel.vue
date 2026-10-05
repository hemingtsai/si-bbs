<script setup lang="ts">
import { onMounted, ref } from 'vue'

import { adminApi } from '../../api'
import type { AdminUser, Role, Stats } from '../../api/types'
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

onMounted(load)
</script>

<template>
  <section>
    <h1>管理后台</h1>

    <dl v-if="stats" class="stats">
      <dt>用户</dt>
      <dd>{{ stats.users }}（封禁 {{ stats.users_banned }}）</dd>
      <dt>项目</dt>
      <dd>{{ stats.projects_approved }} 已通过 / {{ stats.projects_pending }} 待审核</dd>
      <dt>Wiki</dt>
      <dd>{{ stats.wiki_published }} 已发布</dd>
      <dt>评论</dt>
      <dd>{{ stats.comments }}</dd>
      <dt>评分</dt>
      <dd>{{ stats.ratings }}</dd>
      <dt>回收站</dt>
      <dd>{{ stats.trashed }}</dd>
    </dl>

    <form class="filters" @submit.prevent="refine">
      <input v-model="q" type="search" placeholder="搜索用户名或邮箱" />
      <select v-model="roleFilter" @change="refine">
        <option value="">全部角色</option>
        <option value="admin">admin</option>
        <option value="moderator">moderator</option>
        <option value="user">user</option>
      </select>
      <button type="submit">搜索</button>
    </form>

    <p v-if="loading">加载中…</p>
    <p v-else-if="error" class="error">{{ error }}</p>

    <table v-else class="users">
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
            <select :value="user.role" @change="setRole(user, ($event.target as HTMLSelectElement).value as Role)">
              <option value="admin">admin</option>
              <option value="moderator">moderator</option>
              <option value="user">user</option>
            </select>
          </td>
          <td>{{ user.banned ? '已封禁' : '正常' }}</td>
          <td>{{ user.created_at }}</td>
          <td>
            <button @click="toggleBan(user)">{{ user.banned ? '解封' : '封禁' }}</button>
          </td>
        </tr>
      </tbody>
    </table>

    <nav v-if="total > perPage" class="pager">
      <button :disabled="page <= 1" @click="page--; load()">上一页</button>
      <span>{{ page }} / {{ Math.ceil(total / perPage) }}</span>
      <button :disabled="page * perPage >= total" @click="page++; load()">下一页</button>
    </nav>
  </section>
</template>

<style scoped>
.error {
  color: #b00020;
}
.stats {
  display: grid;
  grid-template-columns: auto 1fr;
  gap: 0 1rem;
  max-width: 28rem;
}
.filters {
  display: flex;
  gap: 0.5rem;
  margin: 1rem 0;
}
.users {
  border-collapse: collapse;
  width: 100%;
}
.users th,
.users td {
  border: 1px solid var(--si-border);
  padding: 0.25rem 0.75rem;
  text-align: left;
}
.pager {
  margin-top: 1rem;
  display: flex;
  gap: 1rem;
  align-items: center;
}
</style>
