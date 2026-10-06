import { createHash } from 'node:crypto'
import type { APIRequestContext } from '@playwright/test'

/**
 * Solve a proof-of-work challenge the way the browser does.
 *
 * Tests that drive the API directly (rather than through the UI) still have to do the
 * work — there is deliberately no test-only bypass, so these helpers exercise the
 * same protocol a browser does. Node's crypto is synchronous and fast, which is also
 * why this file is a reminder that the challenge is a speed bump for scripted abuse
 * and not a wall against a determined attacker.
 */
export interface Solution {
  challenge: string
  answer: string
}

function nonceOf(token: string): string {
  const payload = token.split('.')[1]
  if (!payload) throw new Error(`malformed challenge: ${token}`)
  return JSON.parse(Buffer.from(payload, 'base64url').toString('utf8')).nonce as string
}

function leadingZeroBits(digest: Buffer): number {
  let bits = 0
  for (const byte of digest) {
    if (byte === 0) {
      bits += 8
      continue
    }
    bits += Math.clz32(byte) - 24
    break
  }
  return bits
}

export function solveToken(token: string, difficulty: number): string {
  const nonce = nonceOf(token)
  for (let counter = 0; ; counter += 1) {
    const answer = String(counter)
    if (leadingZeroBits(createHash('sha256').update(nonce + answer).digest()) >= difficulty) {
      return answer
    }
  }
}

/**
 * Fetch and solve a challenge. Returns `undefined` when the deployment has it
 * switched off, which is what the request body should carry in that case.
 */
export async function solvePow(ctx: APIRequestContext): Promise<Solution | undefined> {
  const res = await ctx.get('/api/auth/challenge')
  if (!res.ok()) throw new Error(`challenge request failed: ${res.status()}`)
  const body = (await res.json()) as {
    challenge: string
    difficulty: number
    required: boolean
  }
  if (!body.required || body.difficulty === 0) return undefined
  return { challenge: body.challenge, answer: solveToken(body.challenge, body.difficulty) }
}
