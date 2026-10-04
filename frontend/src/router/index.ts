import { createRouter, createWebHistory } from 'vue-router'

export const router = createRouter({
  history: createWebHistory(),
  routes: [
    { path: '/', name: 'home', component: () => import('../views/Home.vue') },
    {
      path: '/wiki',
      name: 'wiki-list',
      component: () => import('../views/wiki/WikiList.vue'),
    },
    {
      path: '/wiki/:slug',
      name: 'wiki-detail',
      component: () => import('../views/wiki/WikiDetail.vue'),
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
      path: '/login',
      name: 'login',
      component: () => import('../views/auth/Login.vue'),
    },
    {
      path: '/register',
      name: 'register',
      component: () => import('../views/auth/Register.vue'),
    },
  ],
})