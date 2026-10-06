import { type APIRequestContext, expect, request, test } from '@playwright/test'

import { promoteUser } from './support/db'

/**
 * End-to-end coverage of the forum API through real HTTP.
 *
 * The database is wiped by `playwright.config.ts` before the backend starts, and
 * every account name carries a per-run suffix, so the suite is repeatable: an
 * earlier version asserted absolute totals against a database that survived
 * between runs and therefore failed the second time it was executed.
 *
 * These cases drive the HTTP surface only. Browser-level flows are not covered
 * yet — see `docs/testing.md`.
 */
const API = 'http://127.0.0.1:3000'
// Fixed per-run-suffix instead of a timestamp: Playwright evaluates a test module in
// several processes, so a module-level `Date.now()` differs between tests of the same
// file, and a test that signs in as a user an earlier test created would get a 401.
// The database is wiped before every run (see `support/reset-db.mjs`), so names only
// need to be unique *within* a run.
const RUN = 'api'
const PASSWORD = 'password123'

// Distinct account per case: registration is case-insensitively unique, so tests
// must not share a name.
const A1 = `a1_${RUN}`
const A2 = `a2_${RUN}`
const B1 = `b1_${RUN}`
const M1 = `m1_${RUN}`
const A3 = `a3_${RUN}`
const M2 = `m2_${RUN}`

async function api(): Promise<APIRequestContext> {
  return request.newContext({ baseURL: API })
}

async function register(ctx: APIRequestContext, username: string): Promise<void> {
  const res = await ctx.post('/api/auth/register', {
    data: { username, email: `${username}@e2e.test`, password: PASSWORD },
  })
  expect(res.status(), `register ${username}`).toBe(201)
}

async function login(ctx: APIRequestContext, username: string): Promise<string> {
  const res = await ctx.post('/api/auth/login', {
    data: { username, password: PASSWORD },
  })
  expect(res.status(), `login ${username}`).toBe(200)
  return (await res.json()).access_token
}

async function postCount(ctx: APIRequestContext, board: string): Promise<number> {
  const res = await ctx.get(`/api/forum/posts?board=${board}&per_page=1`)
  expect(res.status()).toBe(200)
  return (await res.json()).total
}

async function createPost(
  ctx: APIRequestContext,
  auth: Record<string, string>,
  board: string,
  title: string,
): Promise<number> {
  const res = await ctx.post('/api/forum/posts', {
    data: { board, title, content: '内容' },
    headers: auth,
  })
  expect(res.status()).toBe(201)
  return (await res.json()).id
}

test('发帖与回帖不经审核即公开，点赞可切换', async () => {
  const alice = await api()
  await register(alice, A1)
  const auth = { Authorization: `Bearer ${await login(alice, A1)}` }

  const before = await postCount(alice, 'models')
  const postId = await createPost(alice, auth, 'models', `E2E 帖子 ${RUN}`)

  // Visible immediately: there is no pending state at all.
  expect(await postCount(alice, 'models')).toBe(before + 1)

  // Toggle: like -> unlike -> like.
  for (const expected of [1, 0, 1]) {
    const res = await alice.post(`/api/forum/posts/${postId}/like`, { headers: auth })
    expect(res.status()).toBe(200)
    expect((await res.json()).likes_count).toBe(expected)
  }

  const reply = await alice.post(`/api/forum/posts/${postId}/comments`, {
    data: { content: '沙发' },
    headers: auth,
  })
  expect(reply.status()).toBe(201)

  const detail = await (await alice.get(`/api/forum/posts/${postId}`)).json()
  expect(detail.comments_count).toBe(1)
  expect(detail.likes_count).toBe(1)
})

test('作者可删自己的帖子，他人越权被拒，版主可删任何帖子', async () => {
  const alice = await api()
  const bob = await api()
  const mod = await api()
  await register(alice, A2)
  await register(bob, B1)
  await register(mod, M1)
  const aliceAuth = { Authorization: `Bearer ${await login(alice, A2)}` }
  const bobAuth = { Authorization: `Bearer ${await login(bob, B1)}` }
  // Logged in *before* being promoted: the permission must come from the
  // database on every request, not from what the token says.
  const modAuth = { Authorization: `Bearer ${await login(mod, M1)}` }
  promoteUser(M1, 'moderator')

  const forMod = await createPost(alice, aliceAuth, 'life', `版主可删 ${RUN}`)
  const bobDelete = await bob.delete(`/api/forum/posts/${forMod}`, { headers: bobAuth })
  expect(bobDelete.status()).toBe(403)
  const modDelete = await mod.delete(`/api/forum/posts/${forMod}`, { headers: modAuth })
  expect(modDelete.status()).toBe(204)

  const own = await createPost(alice, aliceAuth, 'life', `作者自删 ${RUN}`)
  const ownDelete = await alice.delete(`/api/forum/posts/${own}`, { headers: aliceAuth })
  expect(ownDelete.status()).toBe(204)
})

test('精选帖排在列表最前，板规按版块返回', async () => {
  const alice = await api()
  const mod = await api()
  await register(alice, A3)
  await register(mod, M2)
  const aliceAuth = { Authorization: `Bearer ${await login(alice, A3)}` }
  const modAuth = { Authorization: `Bearer ${await login(mod, M2)}` }
  promoteUser(M2, 'moderator')

  const before = await postCount(alice, 'tools')
  // The featured post is created *first*, so it has the smaller id: if the
  // listing ignored `is_featured` it would sort behind the newer plain post and
  // this assertion would catch it. (Creating it second would pass even with the
  // feature removed.)
  const featuredTitle = `被精选 ${RUN}`
  const featured = await createPost(alice, aliceAuth, 'tools', featuredTitle)
  await createPost(alice, aliceAuth, 'tools', `普通 ${RUN}`)

  const res = await mod.patch(`/api/forum/posts/${featured}/featured`, {
    data: { featured: true },
    headers: modAuth,
  })
  expect(res.status()).toBe(200)

  const list = await (await alice.get('/api/forum/posts?board=tools&per_page=100')).json()
  expect(list.total).toBe(before + 2)
  const ours = list.items
    .map((p: { title: string }) => p.title)
    .filter((t: string) => t.endsWith(RUN))
  expect(ours[0]).toBe(featuredTitle)

  const rules = await (await alice.get('/api/forum/rules?board=tools')).json()
  expect(rules.map((r: { board: string }) => r.board)).toEqual(['global', 'tools'])
})
