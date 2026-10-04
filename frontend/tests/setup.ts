import { afterEach } from 'vitest'

// jsdom's localStorage can be shadowed by Node's experimental native
// implementation, which is undefined unless --localstorage-file is passed.
afterEach(() => {
  try {
    localStorage.clear()
  } catch {
    // no localStorage in this environment; nothing to reset
  }
})