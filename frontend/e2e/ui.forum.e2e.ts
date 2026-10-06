import { type APIRequestContext, expect, request, test, type Page } from '@playwright/test'

import { promoteUser } from './support/db'
import { solvePow } from './support/pow'

/**
 * Browser-level coverage of the forum.
 *
 * The API suite (`forum.e2e.ts`) proves the HTTP contract; these cases drive the
 * actual Vue app through chromium, which is the only way to catch a broken
 * template, a stale store, or a route that renders nothing. They run against the
 * production build served by `vite preview`, with the API proxied to the backend.
 *
 * The database is wiped per run (see `playwright.config.ts`), so every account
 * name carries a per-run suffix.
 */
const API = 'http://127.0.0.1:3000'
// Stable suffix, not a timestamp: see the note in `forum.e2e.ts`. Tests in this file
// reuse accounts created by earlier tests, which only works if every evaluation of
// this module agrees on the names.
const RUN = 'uif'
const PASSWORD = 'password123'

const AUTHOR = `ua_${RUN}`
const DELETER = `ud_${RUN}`
const STAFF = `us_${RUN}`

async function api(): Promise<APIRequestContext> {
  return request.newContext({ baseURL: API })
}

async function registerAndLogin(page: Page, username: string): Promise<void> {
  await page.goto('/register')
  await page.getByLabel('用户名').fill(username)
  await page.getByLabel('邮箱').fill(`${username}@e2e.test`)
  await page.getByLabel('密码', { exact: true }).fill(PASSWORD)
  await page.getByLabel('确认密码').fill(PASSWORD)
  await page.getByRole('button', { name: '注册', exact: true }).click()

  // The register view shows a notice and forwards to the login form.
  await expect(page).toHaveURL(/\/login/, { timeout: 10_000 })
  await page.getByLabel('用户名').fill(username)
  await page.getByLabel('密码', { exact: true }).fill(PASSWORD)
  await page.getByRole('button', { name: '登录', exact: true }).click()
  await expect(page).toHaveURL('/')
  // The sidebar renders the store's username, so this also proves the login
  // response was persisted into the Pinia store, not just localStorage.
  await expect(page.locator('.sidebar')).toContainText(username)
}

/** Create a post through the API and return its id. */
async function createPostViaApi(
  token: string,
  title: string,
  board = 'tools',
): Promise<number> {
  const ctx = await api()
  try {
    const res = await ctx.post('/api/forum/posts', {
      data: { board, title, content: '正文', pow: await solvePow(ctx) },
      headers: { Authorization: `Bearer ${token}` },
    })
    expect(res.status()).toBe(201)
    return (await res.json()).id
  } finally {
    await ctx.dispose()
  }
}

async function apiToken(username: string): Promise<string> {
  const ctx = await api()
  try {
    const res = await ctx.post('/api/auth/login', {
      data: { username, password: PASSWORD, pow: await solvePow(ctx) },
    })
    if (res.status() !== 200) {
      throw new Error(`login for ${username} failed: ${res.status()} ${await res.text()}`)
    }
    return (await res.json()).access_token
  } finally {
    await ctx.dispose()
  }
}

test('浏览器里走完注册 → 发帖 → 点赞 → 回复 → 列表可见', async ({ page }) => {
  await registerAndLogin(page, AUTHOR)

  // Sidebar navigation into the forum.
  await page.getByRole('link', { name: '论坛' }).click()
  await expect(page).toHaveURL('/forum')
  await expect(page.getByRole('heading', { name: '论坛' })).toBeVisible()

  // The rules block is seeded, so it renders without any setup.
  await expect(page.getByText('总站规与板规')).toBeVisible()

  await page.getByRole('link', { name: '发帖' }).click()
  await expect(page).toHaveURL('/forum/new')

  const title = `UI 帖子 ${RUN}`
  await page.getByLabel('板块').selectOption('tools')
  await page.getByLabel('标题').fill(title)
  await page.getByLabel(/正文/).fill('来自浏览器的正文')
  await page.getByRole('button', { name: '发送' }).click()

  // The create view redirects to the new post's detail page.
  await expect(page).toHaveURL(/\/forum\/\d+/)
  await expect(page.getByRole('heading', { name: title })).toBeVisible()
  await expect(page.getByText('来自浏览器的正文')).toBeVisible()

  // Like toggles in the UI: the button starts at zero and the server's answer is
  // what gets rendered.
  const like = page.getByRole('button', { name: /♥/ }).first()
  await expect(like).toHaveText(/♥\s*0/)
  await like.click()
  await expect(like).toHaveText(/♥\s*1/)

  // Reply without any moderation step.
  await page.getByPlaceholder('写下你的回复').fill('沙发')
  await page.getByRole('button', { name: '回复' }).click()
  await expect(page.getByText('沙发')).toBeVisible()
  await expect(page.getByText('回复（1 / 1）')).toBeVisible()

  // And the list reflects both counters.
  await page.getByRole('link', { name: '论坛' }).click()
  const row = page.getByRole('link', { name: new RegExp(title) })
  await expect(row).toBeVisible()
  await expect(row).toContainText('♥ 1')
  await expect(row).toContainText('↩ 1')
})

test('未登录访客能看到内容，但看不到点赞入口，只看到登录提示', async ({ page }) => {
  // Its own account: leaning on a user another test created made this case fail
  // whenever it was run on its own (`-g`), which is a confusing way to find out.
  const owner = `uv_${RUN}`
  await registerAndLogin(page, owner)
  const token = await apiToken(owner)
  const id = await createPostViaApi(token, `匿名可见 ${RUN}`)

  // Back to being a stranger: same browser context, no session.
  await page.getByRole('button', { name: '退出' }).click()

  await page.goto(`/forum/${id}`)
  await expect(page.getByRole('heading', { name: `匿名可见 ${RUN}` })).toBeVisible()
  await expect(page.getByText('正文')).toBeVisible()

  // The like control and the reply form are behind `auth.isAuthenticated`.
  await expect(page.getByRole('button', { name: /♥/ })).toHaveCount(0)
  await expect(page.getByPlaceholder('写下你的回复')).toHaveCount(0)
  await expect(page.getByText('后才能回复。')).toBeVisible()
})

test('作者在页面上删掉自己的帖子，列表里随之消失', async ({ page }) => {
  // Its own account: registration is case-insensitively unique, so tests must not
  // lean on a user another test created.
  await registerAndLogin(page, DELETER)
  const token = await apiToken(DELETER)
  const title = `UI 待删 ${RUN}`
  const id = await createPostViaApi(token, title)

  await page.goto(`/forum/${id}`)
  await expect(page.getByRole('heading', { name: title })).toBeVisible()

  // The delete button asks for confirmation before doing anything.
  page.on('dialog', (dialog) => dialog.accept())
  await page.getByRole('button', { name: '删除' }).click()

  await expect(page).toHaveURL('/forum')
  await expect(page.getByRole('link', { name: new RegExp(title) })).toHaveCount(0)
})

/// The role lives in the store, and the store reconciles with the server on app
/// boot. Promoting in the database therefore shows up after a reload — which is
/// exactly the behaviour that used to be missing when `fetchMe()` was never
/// called.
test('被提权为版主后刷新页面，才出现审核入口与精选按钮', async ({ page }) => {
  await registerAndLogin(page, STAFF)
  const token = await apiToken(STAFF)
  const id = await createPostViaApi(token, `精选候选 ${RUN}`)

  // Before the promotion there is no staff entry point.
  await expect(page.getByRole('link', { name: '审核' })).toHaveCount(0)

  promoteUser(STAFF, 'moderator')
  await page.reload()

  await expect(page.getByRole('link', { name: '审核' })).toBeVisible()
  await page.goto(`/forum/${id}`)
  await page.getByRole('button', { name: '设为精选' }).click()
  await expect(page.getByRole('button', { name: '取消精选' })).toBeVisible()
})
