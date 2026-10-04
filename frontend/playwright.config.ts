import { defineConfig, devices } from '@playwright/test'

const HOST = '127.0.0.1'
const PORT = 5173
const REPO_ROOT = '..'
const BASE_URL = `http://${HOST}:${PORT}`
const API_URL = `http://${HOST}:3000`
const HEALTH_URL = `${API_URL}/api/health`

const BACKEND_CMD = 'cargo run --manifest-path backend/Cargo.toml'
const PREVIEW_BUILD = 'npm --prefix frontend run build:only'
const PREVIEW_ARGS = `--port ${PORT} --strictPort --host ${HOST}`
const PREVIEW_SERVE = `npm --prefix frontend run preview -- ${PREVIEW_ARGS}`
const PREVIEW = `${PREVIEW_BUILD} && ${PREVIEW_SERVE}`
const BACKEND_ENV = {
  DATABASE_URL: 'sqlite://si-bbs-e2e.db?mode=rwc',
  GITHUB_MOCK: 'true',
  JWT_SECRET: 'e2e-test-secret',
}
const CI = !!process.env.CI
const WEB_SERVER = {
  // cwd is the repo root so `cargo run --manifest-path backend/Cargo.toml` and
  // `npm --prefix frontend` both resolve.
  cwd: REPO_ROOT,
  reuseExistingServer: !CI,
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