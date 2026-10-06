/**
 * Human verification (proof of work), solved in the browser.
 *
 * The server hands out a signed challenge and expects a counter whose
 * `sha256(nonce ‖ counter)` has a run of leading zero bits. The work happens here so
 * that a bot has to run this code per attempt instead of just replaying a form POST.
 *
 * Two deliberate properties:
 *
 * - **Nothing is shown unless it takes a moment.** An honest user submits a form and
 *   gets an answer; the only difference is a fraction of a second of latency, and a
 *   `solving` flag the views may surface if it is slow.
 * - **A refused challenge is retried once, transparently.** Challenges expire and are
 *   single-use, so a stale one is an expected event, not an error worth showing.
 */
import { api } from '../api/axios'

interface ChallengeResponse {
  challenge: string
  difficulty: number
  expires_in_secs: number
  required: boolean
}

export interface Solution {
  challenge: string
  answer: string
}

/** Set while a challenge is being solved, so a view can say "verifying…". */
let solvingCount = 0
const listeners = new Set<(solving: boolean) => void>()

export function onSolvingChange(listener: (solving: boolean) => void): () => void {
  listeners.add(listener)
  return () => listeners.delete(listener)
}

export function isSolving(): boolean {
  return solvingCount > 0
}

function setSolving(delta: number): void {
  solvingCount += delta
  for (const listener of listeners) {
    listener(solvingCount > 0)
  }
}

/** Read the `nonce` out of the challenge token's payload.
 *
 * The signature is the server's business — it verifies it — but the nonce is what the
 * client has to hash against, and it is in the (base64url) middle segment.
 */
export function nonceOf(token: string): string {
  const payload = token.split('.')[1]
  if (!payload) throw new Error('malformed challenge')
  const padded = payload.replace(/-/g, '+').replace(/_/g, '/')
  const json = decodeURIComponent(
    atob(padded + '='.repeat((4 - (padded.length % 4)) % 4))
      .split('')
      .map((c) => `%${c.charCodeAt(0).toString(16).padStart(2, '0')}`)
      .join(''),
  )
  const nonce = (JSON.parse(json) as { nonce?: string }).nonce
  if (!nonce) throw new Error('challenge carries no nonce')
  return nonce
}

/** Leading zero bits of a digest, matching the server's count. */
export function leadingZeroBits(digest: ArrayBuffer): number {
  const bytes = new Uint8Array(digest)
  let bits = 0
  for (const byte of bytes) {
    if (byte === 0) {
      bits += 8
      continue
    }
    bits += Math.clz32(byte) - 24
    break
  }
  return bits
}

/**
 * Find a counter that meets the difficulty.
 *
 * Each digest is awaited, so the expensive part yields to the event loop between
 * iterations: the page keeps painting and Vue keeps updating the "verifying" state
 * instead of freezing for the duration.
 */
export async function solve(token: string, difficulty: number): Promise<string> {
  const nonce = nonceOf(token)
  const encoder = new TextEncoder()
  for (let counter = 0; counter < Number.MAX_SAFE_INTEGER; counter += 1) {
    const answer = String(counter)
    const digest = await crypto.subtle.digest('SHA-256', encoder.encode(nonce + answer))
    if (leadingZeroBits(digest) >= difficulty) {
      return answer
    }
  }
  throw new Error('no solution found')
}

async function fetchChallenge(): Promise<ChallengeResponse> {
  const { data } = await api.get<ChallengeResponse>('/auth/challenge')
  return data
}

/** True when the failure is the server asking for (fresh) proof of work. */
function isChallengeError(err: unknown): boolean {
  const data = (err as { response?: { data?: { code?: string } } })?.response?.data
  return data?.code === 'pow'
}

/**
 * Run `send` with a solved challenge, retrying once with a fresh one if the server
 * refuses the proof (expired or already spent).
 *
 * When the deployment has the challenge switched off, `send` is called without a
 * `pow` field and nothing else changes.
 */
export async function withPow<T>(
  send: (pow: Solution | undefined) => Promise<T>,
): Promise<T> {
  const challenge = await fetchChallenge()
  if (!challenge.required || challenge.difficulty === 0) {
    return send(undefined)
  }

  setSolving(1)
  let solution: Solution
  try {
    solution = {
      challenge: challenge.challenge,
      answer: await solve(challenge.challenge, challenge.difficulty),
    }
  } finally {
    setSolving(-1)
  }

  try {
    return await send(solution)
  } catch (err) {
    if (!isChallengeError(err)) throw err
    // Expired, replayed, or solved against a challenge the server no longer accepts:
    // get a new one and try once more. A second refusal is a real error.
    const retry = await fetchChallenge()
    if (!retry.required || retry.difficulty === 0) {
      return send(undefined)
    }
    setSolving(1)
    try {
      return await send({
        challenge: retry.challenge,
        answer: await solve(retry.challenge, retry.difficulty),
      })
    } finally {
      setSolving(-1)
    }
  }
}
