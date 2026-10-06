import { createRouter, createWebHistory, type RouteLocationNormalized } from 'vue-router'

export const router = createRouter({
  history: createWebHistory(),
  routes: [
    { path: '/', name: 'home', component: () => import('../views/Home.vue') },
    { path: '/wiki', name: 'wiki-list', component: () => import('../views/wiki/WikiList.vue') },
    {
      path: '/wiki/:slug',
      name: 'wiki-detail',
      component: () => import('../views/wiki/WikiDetail.vue'),
    },
    {
      path: '/wiki/:slug/edit',
      name: 'wiki-edit',
      component: () => import('../views/wiki/WikiEditor.vue'),
      meta: { requiresAuth: true },
    },
    {
      path: '/wiki/new',
      name: 'wiki-create',
      component: () => import('../views/wiki/WikiEditor.vue'),
      meta: { requiresAuth: true },
    },
    {
      path: '/projects',
      name: 'project-list',
      component: () => import('../views/projects/ProjectList.vue'),
    },
    {
      path: '/projects/:id',
      name: 'project-detail',
      component: () => import('../views/projects/ProjectDetail.vue'),
    },
    {
      path: '/forum',
      name: 'forum',
      component: () => import('../views/forum/ForumList.vue'),
    },
    {
      path: '/forum/new',
      name: 'forum-new',
      component: () => import('../views/forum/ForumNew.vue'),
      meta: { requiresAuth: true },
    },
    {
      path: '/forum/:id',
      name: 'forum-detail',
      component: () => import('../views/forum/ForumDetail.vue'),
    },
    {
      path: '/projects/submit',
      name: 'project-submit',
      component: () => import('../views/projects/ProjectSubmit.vue'),
      meta: { requiresAuth: true },
    },
    { path: '/me', name: 'mine', component: () => import('../views/Mine.vue'), meta: { requiresAuth: true } },
    {
      path: '/moderation',
      name: 'moderation',
      component: () => import('../views/Moderation.vue'),
      meta: { requiresAuth: true, requiresStaff: true },
    },
    { path: '/trash', name: 'trash', component: () => import('../views/Trash.vue'), meta: { requiresAuth: true } },
    {
      path: '/admin',
      name: 'admin',
      component: () => import('../views/admin/AdminPanel.vue'),
      meta: { requiresAuth: true, requiresAdmin: true },
    },
    { path: '/login', name: 'login', component: () => import('../views/auth/Login.vue') },
    { path: '/register', name: 'register', component: () => import('../views/auth/Register.vue') },
    { path: '/:pathMatch(.*)*', redirect: { name: 'home' } },
  ],
})

router.beforeEach((to: RouteLocationNormalized) => {
  // Tokens are all we can check synchronously; lazy checks hit the API.
  const authed = localStorage.getItem('access_token') !== null
  const role = localStorage.getItem('user_role')

  if (to.meta.requiresAuth && !authed) {
    return { name: 'login', query: { redirect: to.fullPath } }
  }
  if (to.meta.requiresAdmin && role !== 'admin') {
    return { name: 'home' }
  }
  if (to.meta.requiresStaff && role !== 'admin' && role !== 'moderator') {
    return { name: 'home' }
  }
  if ((to.name === 'login' || to.name === 'register') && authed) {
    return { name: 'home' }
  }
  return true
})