import { expect, request, test, type Page } from '@playwright/test'

/**
 * Browser-level coverage of the account self-service page.
 *
 * Everything here is end-to-end on purpose: the password change returns a *new*
 * token pair because the server invalidates the old one, so a page that forgets to
 * store it would look fine until the next navigation — which is exactly what this
 * test walks through.
 */
const API = 'http://127.0.0.1:3000'
const RUN = Date.now().toString(36)
const PASSWORD = 'password123'

const USER = `up_${RUN}`

async function registerAndLogin(page: Page, username: string): Promise<void> {
  await page.goto('/register')
  await page.getByLabel('用户名').fill(username)
  await page.getByLabel('邮箱').fill(`${username}@e2e.test`)
  await page.getByLabel('密码', { exact: true }).fill(PASSWORD)
  await page.getByLabel('确认密码').fill(PASSWORD)
  await page.getByRole('button', { name: '注册', exact: true }).click()

  await expect(page).toHaveURL(/\/login/, { timeout: 10_000 })
  await page.getByLabel('用户名').fill(username)
  await page.getByLabel('密码', { exact: true }).fill(PASSWORD)
  await page.getByRole('button', { name: '登录', exact: true }).click()
  await expect(page).toHaveURL('/')
}

test('资料保存后侧栏立刻显示昵称，改密码后当前会话仍然可用', async ({ page }) => {
  await registerAndLogin(page, USER)

  // The login name is what the shell shows until a display name exists.
  await expect(page.locator('.sidebar')).toContainText(USER)

  await page.goto('/settings')
  await expect(page.getByRole('heading', { name: '个人设置' })).toBeVisible()

  const nickname = `昵称_${RUN}`
  await page.getByLabel('昵称').fill(nickname)
  await page.getByLabel('简介').fill('一句话简介')
  await page.getByRole('button', { name: '保存资料' }).click()
  await expect(page.getByText('已保存')).toBeVisible()
  // The store is updated from the response, so the sidebar follows immediately.
  await expect(page.locator('.sidebar')).toContainText(nickname)

  // Reload: the display name has to survive because the account payload is read
  // again on boot, not because the page kept it in memory.
  await page.reload()
  await expect(page.locator('.sidebar')).toContainText(nickname)

  // Change the password. This invalidates every token the server ever issued,
  // including the one this tab is using, so the page must store the new pair.
  await page.getByLabel('当前密码').first().fill(PASSWORD)
  await page.getByLabel('新密码', { exact: true }).fill('brand-new-password')
  await page.getByLabel('确认新密码').fill('brand-new-password')
  await page.getByRole('button', { name: '修改密码' }).click()
  await expect(page.getByText('密码已更新，其它设备上的登录已失效')).toBeVisible()

  // Navigating triggers a fresh API call with the new token: an hour of debugging
  // saved if the pair were not persisted.
  await page.goto('/me')
  await expect(page).toHaveURL('/me')
  await expect(page.getByText('登录')).toHaveCount(0)

  // The new password really is the password.
  await page.getByRole('button', { name: '退出' }).click()
  await page.goto('/login')
  await page.getByLabel('用户名').fill(USER)
  await page.getByLabel('密码', { exact: true }).fill('brand-new-password')
  await page.getByRole('button', { name: '登录', exact: true }).click()
  await expect(page).toHaveURL('/')
})

test('资料校验与我的举报在页面上可见', async ({ page }) => {
  const reporter = `ur_${RUN}`
  await registerAndLogin(page, reporter)

  await page.goto('/settings')
  // A javascript: avatar is refused by the server, and the page says so instead of
  // pretending it saved.
  await page.getByLabel('头像地址').fill('javascript:alert(1)')
  await page.getByRole('button', { name: '保存资料' }).click()
  await expect(page.getByText(/保存失败/)).toBeVisible()

  // Wrong current password is reported as such.
  await page.getByLabel('当前密码').first().fill('definitely-not-it')
  await page.getByLabel('新密码', { exact: true }).fill('another-password')
  await page.getByLabel('确认新密码').fill('another-password')
  await page.getByRole('button', { name: '修改密码' }).click()
  await expect(page.getByText('当前密码不正确')).toBeVisible()

  // Mismatched confirmation is caught locally.
  await page.getByLabel('当前密码').first().fill(PASSWORD)
  await page.getByLabel('新密码', { exact: true }).fill('another-password')
  await page.getByLabel('确认新密码').fill('mismatched-password')
  await page.getByRole('button', { name: '修改密码' }).click()
  await expect(page.getByText('两次输入的新密码不一致')).toBeVisible()

  await expect(page.getByText('还没有提交过举报')).toBeVisible()
})

test('Wiki 历史能对比并回滚', async ({ page }) => {
  const author = `uw_${RUN}`
  await registerAndLogin(page, author)

  // Create a page through the real editor.
  await page.goto('/wiki/new')
  await page.getByLabel('标题').fill(`历史页面_${RUN}`)
  await page.getByLabel('分类').fill('测试')
  await page.getByLabel('正文（Markdown）').fill('第一版正文')
  await page.getByLabel('链接别名（slug）').fill(`history-${RUN}`)
  await page.getByRole('button', { name: '保存' }).click()
  await expect(page).toHaveURL(new RegExp(`/wiki/history-${RUN}$`))
  await expect(page.getByText('第一版正文')).toBeVisible()

  // Second revision, with a note.
  await page.getByRole('link', { name: '编辑' }).click()
  await page.getByLabel('正文（Markdown）').fill('第二版正文')
  await page.getByLabel('修改说明（可选）').fill('补充内容')
  await page.getByRole('button', { name: '保存' }).click()
  await expect(page).toHaveURL(new RegExp(`/wiki/history-${RUN}$`))

  // History lists both versions and compares them.
  await page.getByRole('link', { name: /历史/ }).click()
  await expect(page).toHaveURL(new RegExp(`/wiki/history-${RUN}/history$`))
  await expect(page.getByText('第 2 版', { exact: true })).toBeVisible()
  await expect(page.getByText('补充内容')).toBeVisible()
  await page.getByRole('button', { name: '对比上一版' }).first().click()
  await expect(page).toHaveURL(/from=1&to=2|to=2&from=1/)
  await expect(page.locator('.diff')).toContainText('第二版正文')
  await expect(page.locator('.diff .line.remove')).toContainText('第一版正文')

  // Roll back to revision 1: history grows instead of being rewritten.
  page.once('dialog', (dialog) => dialog.accept())
  await page.getByRole('button', { name: '回滚到此版' }).click()
  await expect(page.getByText(/已恢复为第 1 版/)).toBeVisible()
  await expect(page.getByText('第 3 版', { exact: true })).toBeVisible()
  await page.getByRole('link', { name: '返回页面' }).click()
  await expect(page.getByText('第一版正文')).toBeVisible()
})

/**
 * The concurrency guard is only real if the *editor* sends the revision it started
 * from. This drives that path: the page is changed behind the open editor's back,
 * and the save must be refused rather than silently overwriting the other edit.
 */
test('陈旧编辑被拒绝并保留输入，而不是静默覆盖', async ({ page }) => {
  const author = `uc_${RUN}`
  await registerAndLogin(page, author)

  await page.goto('/wiki/new')
  await page.getByLabel('标题').fill(`并发页面_${RUN}`)
  await page.getByLabel('分类').fill('测试')
  await page.getByLabel('正文（Markdown）').fill('初始正文')
  await page.getByLabel('链接别名（slug）').fill(`conflict-${RUN}`)
  // Published: the second editor in this test reads the page through the public API.
  await page.getByLabel('状态').selectOption('published')
  await page.getByRole('button', { name: '保存' }).click()
  await expect(page).toHaveURL(new RegExp(`/wiki/conflict-${RUN}$`))

  // Open the editor: it loads revision 1 as its base.
  await page.getByRole('link', { name: '编辑' }).click()
  await expect(page.getByLabel('正文（Markdown）')).toHaveValue('初始正文')
  await page.getByLabel('正文（Markdown）').fill('我这一版想写的内容')

  // Somebody else (or another tab) saves first.
  const ctx = await request.newContext({ baseURL: API })
  const login = await ctx.post('/api/auth/login', {
    data: { username: author, password: PASSWORD },
  })
  const { access_token: token } = (await login.json()) as { access_token: string }
  const detail = await ctx.get(`/api/wiki/conflict-${RUN}`)
  const pageId = ((await detail.json()) as { id: number }).id
  const other = await ctx.put(`/api/wiki/page/${pageId}`, {
    headers: { Authorization: `Bearer ${token}` },
    data: {
      title: `并发页面_${RUN}`,
      category: '测试',
      content: '别人先保存的正文',
      status: 'published',
      base_revision: 1,
    },
  })
  if (other.status() !== 200) {
    throw new Error(`unexpected ${other.status()}: ${await other.text()}`)
  }
  await ctx.dispose()

  // The stale save is refused with an explanation, and the text is not lost.
  await page.getByRole('button', { name: '保存' }).click()
  await expect(page.getByText(/有人在你编辑期间保存了这个页面/)).toBeVisible()
  await expect(page.getByLabel('正文（Markdown）')).toHaveValue('我这一版想写的内容')
  await expect(page).toHaveURL(new RegExp(`/wiki/conflict-${RUN}/edit$`))

  // The other person's version is what is stored.
  await page.goto(`/wiki/conflict-${RUN}`)
  await expect(page.getByText('别人先保存的正文')).toBeVisible()
})
