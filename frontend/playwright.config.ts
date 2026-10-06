import { defineConfig, devices } from '@playwright/test'

import { resolve } from 'node:path'

import { E2E_DB_URL } from './e2e/support/db'

const HOST = '127.0.0.1'
const PORT = 5173
const REPO_ROOT = '..'
const BASE_URL = `http://${HOST}:${PORT}`
const API_URL = `http://${HOST}:3000`
const HEALTH_URL = `${API_URL}/api/health`

// The database is wiped by the command that starts the backend (see
// `reset-db.mjs`): it has to happen exactly once, right before the server opens
// the file, and the config module itself is evaluated by every worker process.
const BACKEND_CMD =
  'node frontend/e2e/support/reset-db.mjs && cargo run --manifest-path backend/Cargo.toml'
const PREVIEW_BUILD = 'npm --prefix frontend run build:only'
const PREVIEW_ARGS = `--port ${PORT} --strictPort --host ${HOST}`
const PREVIEW_SERVE = `npm --prefix frontend run preview -- ${PREVIEW_ARGS}`
const PREVIEW = `${PREVIEW_BUILD} && ${PREVIEW_SERVE}`
const E2E_UPLOAD_DIR = resolve(REPO_ROOT, '.playwright/uploads')
const BACKEND_ENV = {
  DATABASE_URL: E2E_DB_URL,
  JWT_SECRET: 'e2e-test-secret',
  // Keep test uploads inside the scratch directory: the default (`./uploads`) put
  // them in the repository root, where they showed up as untracked files.
  UPLOAD_DIR: E2E_UPLOAD_DIR,
  // Turn human verification on so the browser flows exercise the real solver, and
  // keep the production difficulty: the suite then measures what a user actually
  // waits for instead of a number nobody runs.
  POW_REQUIRED: 'true',
}
const CI = !!process.env.CI
const WEB_SERVER = {
  // cwd is the repo root so `cargo run --manifest-path backend/Cargo.toml` and
  // `npm --prefix frontend` both resolve.
  cwd: REPO_ROOT,
  // Never adopt a server that is already listening: a running dev instance uses
  // a different database, and silently reusing it meant the suite wrote test
  // users and posts into the developer's own data. Failing loudly on a busy
  // port is the point.
  reuseExistingServer: false,
  timeout: 600_000,
}
const BACKEND_SERVER = { ...WEB_SERVER, env: BACKEND_ENV }
const UI_SERVER = WEB_SERVER

// Playwright waits for each url in declaration order: the backend must be
// listening before the preview server proxies to it.
const SERVERS = [
  { command: BACKEND_CMD, url: HEALTH_URL, ...BACKEND_SERVER },
  { command: PREVIEW, url: BASE_URL, ...UI_SERVER },
]

export default defineConfig({
  testDir: './e2e',
  testMatch: '**/*.e2e.ts',
  outputDir: './test-results',
  fullyParallel: false,
  forbidOnly: CI,
  retries: CI ? 1 : 0,
  reporter: CI ? 'github' : 'list',
  use: {
    baseURL: BASE_URL,
    trace: 'on-first-retry',
    video: 'retain-on-failure',
  },
  projects: [{ name: 'chromium', use: { ...devices['Desktop Chrome'] } }],
  webServer: SERVERS,
})
