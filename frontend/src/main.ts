import { createPinia } from 'pinia'
import { createApp } from 'vue'

import '@fontsource-variable/noto-serif-sc'
import '@fontsource/ibm-plex-mono/400.css'
import './style.css'
import App from './App.vue'
import { router } from './router'
import { useAuthStore } from './stores/auth'

const pinia = createPinia()
createApp(App).use(pinia).use(router).mount('#app')

// Reconcile the cached role with the server before the first paint of any
// role-dependent UI. `fetchMe` was written for this and never called, so a
// demoted moderator kept seeing the moderation entry (and a promoted user did
// not see it) until they logged in again. Failures are ignored: the interceptor
// already clears a dead session, and an offline start should not block the app.
const auth = useAuthStore(pinia)
if (auth.isAuthenticated) {
  void auth.fetchMe().catch(() => {})
}
