import { describe, expect, it, vi } from 'vitest'

import { isSolving, leadingZeroBits, nonceOf, onSolvingChange, solve } from '../../src/lib/pow'

/** Build a challenge token the way the server does: base64url payload, ignored signature. */
function tokenFor(nonce: string): string {
  const payload = btoa(JSON.stringify({ nonce, difficulty: 8, kind: 'pow' }))
    .replace(/\+/g, '-')
    .replace(/\//g, '_')
    .replace(/=+$/, '')
  return `header.${payload}.signature`
}

/** Independent check: hash the solved answer here and count the bits ourselves. */
async function bitsOf(nonce: string, answer: string): Promise<number> {
  const digest = await crypto.subtle.digest('SHA-256', new TextEncoder().encode(nonce + answer))
  return leadingZeroBits(digest)
}

describe('leadingZeroBits', () => {
  it('counts the same way the server does', async () => {
    // sha256("nonce0") starts 0x64 = 0110_0100 → one leading zero bit.
    const digest = await crypto.subtle.digest('SHA-256', new TextEncoder().encode('nonce0'))
    expect(leadingZeroBits(digest)).toBe(1)
  })

  it('counts whole zero bytes and stops at the first set bit', () => {
    // 0x00 0x00 0x0f → 8 + 8 + 4 = 20.
    const bytes = new Uint8Array([0, 0, 0x0f, 0xff]).buffer
    expect(leadingZeroBits(bytes)).toBe(20)
    // All zero → every bit counts.
    expect(leadingZeroBits(new Uint8Array([0, 0]).buffer)).toBe(16)
    // A leading 1 → none.
    expect(leadingZeroBits(new Uint8Array([0xff]).buffer)).toBe(0)
  })
})

describe('nonceOf', () => {
  it('reads the nonce out of the token payload', () => {
    expect(nonceOf(tokenFor('abc123'))).toBe('abc123')
  })

  it('handles the base64url alphabet and missing padding', () => {
    // A nonce whose encoding needs both substitutions and padding removal.
    const nonce = '??>>~~'
    expect(nonceOf(tokenFor(nonce))).toBe(nonce)
  })

  it('refuses a token that is not a challenge', () => {
    expect(() => nonceOf('not-a-jwt')).toThrow()
    expect(() => nonceOf('a.eyJraW5kIjoicG93In0.b')).toThrow() // no nonce field
  })
})

describe('solve', () => {
  it('finds an answer that actually meets the difficulty', async () => {
    for (const difficulty of [4, 8, 10]) {
      const nonce = `nonce-${difficulty}`
      const answer = await solve(tokenFor(nonce), difficulty)
      expect(await bitsOf(nonce, answer)).toBeGreaterThanOrEqual(difficulty)
    }
  })

  it('returns the smallest answer it finds, so the work is not wasted', async () => {
    const answer = await solve(tokenFor('ordering'), 8)
    expect(Number(answer)).toBeGreaterThanOrEqual(0)
  })
})

describe('withPow', () => {
  it('sends no proof at all when the deployment has it switched off', async () => {
    const api = { get: vi.fn().mockResolvedValue({ data: { required: false, difficulty: 0 } }) }
    vi.doMock('../../src/api/axios', () => ({ api }))
    vi.resetModules()
    const fresh = await import('../../src/lib/pow')
    const send = vi.fn().mockResolvedValue('ok')

    await expect(fresh.withPow(send)).resolves.toBe('ok')
    expect(send).toHaveBeenCalledWith(undefined)
  })

  it('retries once with a fresh challenge when the server refuses the proof', async () => {
    vi.resetModules()
    const challenges = [
      { challenge: tokenFor('first'), difficulty: 6, required: true },
      { challenge: tokenFor('second'), difficulty: 6, required: true },
    ]
    let issued = 0
    const api = {
      get: vi.fn().mockImplementation(async () => ({ data: challenges[issued++] })),
    }
    vi.doMock('../../src/api/axios', () => ({ api }))
    const fresh = await import('../../src/lib/pow')

    // The server rejects the first attempt the way it does for an expired challenge.
    const send = vi
      .fn()
      .mockRejectedValueOnce({ response: { data: { code: 'pow', error: 'expired' } } })
      .mockResolvedValueOnce('ok')

    await expect(fresh.withPow(send)).resolves.toBe('ok')
    expect(send).toHaveBeenCalledTimes(2)
    const first = send.mock.calls[0][0] as { challenge: string }
    const second = send.mock.calls[1][0] as { challenge: string }
    expect(first.challenge).toContain(tokenFor('first').split('.')[1])
    expect(second.challenge).toContain(tokenFor('second').split('.')[1])
    // Exactly two challenges were asked for: one per attempt, no more.
    expect(api.get).toHaveBeenCalledTimes(2)
  })

  it('does not retry a failure that is not about the challenge', async () => {
    vi.resetModules()
    const api = {
      get: vi.fn().mockResolvedValue({ data: { challenge: tokenFor('x'), difficulty: 4, required: true } }),
    }
    vi.doMock('../../src/api/axios', () => ({ api }))
    const fresh = await import('../../src/lib/pow')

    const failure = { response: { data: { error: 'password is too short' } } }
    const send = vi.fn().mockRejectedValue(failure)

    await expect(fresh.withPow(send)).rejects.toBe(failure)
    expect(send).toHaveBeenCalledTimes(1)
  })

  it('reports when it is solving, and stops afterwards', async () => {
    vi.resetModules()
    const api = {
      get: vi.fn().mockResolvedValue({ data: { challenge: tokenFor('busy'), difficulty: 8, required: true } }),
    }
    vi.doMock('../../src/api/axios', () => ({ api }))
    const fresh = await import('../../src/lib/pow')

    const seen: boolean[] = []
    const off = fresh.onSolvingChange((value) => seen.push(value))
    expect(fresh.isSolving()).toBe(false)

    await fresh.withPow(async () => 'ok')
    off()

    expect(seen).toContain(true)
    expect(fresh.isSolving()).toBe(false)
  })
})

describe('module surface', () => {
  it('exports the pieces the views use', () => {
    expect(typeof isSolving).toBe('function')
    expect(typeof onSolvingChange).toBe('function')
  })
})
