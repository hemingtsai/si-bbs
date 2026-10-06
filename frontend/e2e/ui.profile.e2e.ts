import { expect, test, type Page } from '@playwright/test'

/**
 * Browser-level coverage of the account self-service page.
 *
 * Everything here is end-to-end on purpose: the password change returns a *new*
 * token pair because the server invalidates the old one, so a page that forgets to
 * store it would look fine until the next navigation — which is exactly what this
 * test walks through.
 */
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
