// Wipes the E2E database directory.
//
// This runs as part of the backend webServer command rather than at the top of
// `playwright.config.ts`: Playwright evaluates the config module in every worker
// process as well as in the runner, so a module-level `rmSync` would delete the
// database out from under the already-running backend and every request that
// touches the database would start failing.
import { mkdirSync, rmSync } from 'node:fs'
import { resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const dir = resolve(fileURLToPath(new URL('.', import.meta.url)), '../../..', '.playwright')
rmSync(dir, { recursive: true, force: true })
mkdirSync(dir, { recursive: true })
console.log(`[e2e] reset database directory ${dir}`)
