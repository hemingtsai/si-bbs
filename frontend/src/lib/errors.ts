/** Extract the backend's `{ "error": "..." }` message, falling back to a generic one. */
export function apiError(err: unknown, fallback: string): string {
  const message = (err as { response?: { data?: { error?: string } } })?.response?.data?.error
  return typeof message === 'string' && message.length > 0 ? message : fallback
}
