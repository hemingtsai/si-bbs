import { createApp } from 'vue'
import { createPinia } from 'pinia'
import '@fontsource-variable/noto-serif-sc'
import '@fontsource/ibm-plex-mono/400.css'
import './style.css'
import App from './App.vue'
import { router } from './router'

createApp(App).use(createPinia()).use(router).mount('#app')
