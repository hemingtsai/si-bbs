import { expect, test } from '@playwright/test'

test('health endpoint responds', async ({ request }) => {
  const res = await request.get('/api/health')
  expect(res.ok()).toBeTruthy()
})