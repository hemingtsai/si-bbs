import { resolve } from 'node:path'
import { DatabaseSync } from 'node:sqlite'
import { fileURLToPath } from 'node:url'

const SUPPORT_DIR = fileURLToPath(new URL('.', import.meta.url))

/** Repository root, reached from `frontend/e2e/support/`. */
export const REPO_ROOT = resolve(SUPPORT_DIR, '../../..')
export const E2E_DB_DIR = resolve(REPO_ROOT, '.playwright')
export const E2E_DB_FILE = resolve(E2E_DB_DIR, 'si-bbs-e2e.db')
export const E2E_DB_URL = `sqlite://${E2E_DB_FILE}?mode=rwc`


/**
 * Grant a role by writing to the database directly.
 *
 * This mirrors the documented way to create the first administrator
 * (`sqlite3 … UPDATE users SET role='admin'`): there is no bootstrap endpoint by
 * design, so a staff fixture has to reach the file. The app re-reads the role on
 * every request, so a token minted before this call picks the new role up
 * immediately — which is exactly what the moderation cases below rely on.
 */
export function promoteUser(username: string, role: 'admin' | 'moderator'): void {
  const db = new DatabaseSync(E2E_DB_FILE)
  try {
    const result = db
      .prepare('UPDATE users SET role = ? WHERE username = ?')
      .run(role, username)
    if (result.changes !== 1) {
      throw new Error(`promoteUser: no user named ${username} (changes=${result.changes})`)
    }
  } finally {
    db.close()
  }
}
