import { api } from './axios'
import type {
  AdminUser,
  ForumPostSummary,
  ProjectSummary,
  WikiPageSummary,
  Comment,
  BoardInfo,
  ForumBoard,
  ForumComment,
  ForumPost,
  ForumRule,
  Me,
  Page,
  Project,
  RateAck,
  RatingSummary,
  Role,
  Stats,
  TrashItem,
  WikiCategory,
  WikiPage,
} from './types'

export const projectsApi = {
  list: (params: {
    category?: string
    q?: string
    sort?: 'stars' | 'recent' | 'name'
    page?: number
    per_page?: number
  }) => api.get<Page<ProjectSummary>>('/projects', { params }),
  detail: (id: number) => api.get<Project>(`/projects/${id}`),
  mine: (params?: { page?: number }) => api.get<Page<ProjectSummary>>('/projects/mine', { params }),
  submit: (payload: { github_url: string; category: string; description?: string }) =>
    api.post<Project>('/projects', payload),
  review: (id: number, action: 'approve' | 'reject', note?: string) =>
    api.post<Project>(`/projects/${id}/review`, { action, note }),
  reviewQueue: (params?: { page?: number }) =>
    api.get<Page<ProjectSummary>>('/projects/review-queue', { params }),
  remove: (id: number) => api.delete<void>(`/projects/${id}`),
}

export const ratingsApi = {
  rate: (projectId: number, score: number, comment?: string) =>
    api.post<RateAck>(`/projects/${projectId}/rating`, { score, comment }),
  summary: (projectId: number) => api.get<RatingSummary>(`/projects/${projectId}/rating/summary`),
}

export const commentsApi = {
  list: (projectId: number, params?: { page?: number; per_page?: number }) =>
    api.get<Page<Comment>>(`/projects/${projectId}/comments`, { params }),
  create: (projectId: number, content: string) =>
    api.post<Comment>(`/projects/${projectId}/comments`, { content }),
  remove: (projectId: number, commentId: number) =>
    api.delete<void>(`/projects/${projectId}/comments/${commentId}`),
}

export const wikiApi = {
  list: (params?: { category?: string; q?: string; page?: number }) =>
    api.get<Page<WikiPageSummary>>('/wiki', { params }),
  detail: (slug: string) => api.get<WikiPage>(`/wiki/${slug}`),
  categories: () => api.get<WikiCategory[]>('/wiki/categories'),
  mine: (params?: { page?: number }) => api.get<Page<WikiPageSummary>>('/wiki/mine', { params }),
  create: (payload: {
    title: string
    category: string
    content: string
    status: 'draft' | 'published'
  }) => api.post<WikiPage>('/wiki', payload),
  update: (
    id: number,
    payload: { title: string; category: string; content: string; status: 'draft' | 'published' },
  ) => api.put<WikiPage>(`/wiki/page/${id}`, payload),
  remove: (id: number) => api.delete<void>(`/wiki/page/${id}`),
}

export const trashApi = {
  list: () => api.get<TrashItem[]>('/trash'),
  restore: (kind: TrashItem['kind'], id: number) =>
    api.post<{ restored: boolean }>(`/trash/${kind}/${id}/restore`),
  purge: (kind: TrashItem['kind'], id: number) => api.delete<void>(`/trash/${kind}/${id}`),
}

export const adminApi = {
  users: (params?: { q?: string; role?: Role; page?: number; per_page?: number }) =>
    api.get<Page<AdminUser>>('/admin/users', { params }),
  setRole: (id: number, role: Role) =>
    api.patch<AdminUser>(`/admin/users/${id}/role`, { role }),
  setBan: (id: number, banned: boolean) =>
    api.patch<AdminUser>(`/admin/users/${id}/ban`, { banned }),
  stats: () => api.get<Stats>('/admin/stats'),
}

export const authApi = {
  me: () => api.get<Me>('/auth/me'),
  login: (payload: { username: string; password: string }) =>
    api.post<import('./types').AuthTokens>('/auth/login', payload),
  register: (payload: { username: string; email: string; password: string }) =>
    api.post<{ role: Role }>('/auth/register', payload),
}

export type { Me }
export const forumApi = {
  boards: () => api.get<BoardInfo[]>('/forum/boards'),
  list: (params?: { board?: string; q?: string; page?: number; per_page?: number }) =>
    api.get<Page<ForumPostSummary>>('/forum/posts', { params }),
  detail: (id: number) => api.get<ForumPost>(`/forum/posts/${id}`),
  create: (payload: { board: ForumBoard; title: string; content: string }) =>
    api.post<ForumPost>('/forum/posts', payload),
  update: (id: number, payload: { board: ForumBoard; title: string; content: string }) =>
    api.patch<ForumPost>(`/forum/posts/${id}`, payload),
  remove: (id: number) => api.delete<void>(`/forum/posts/${id}`),
  like: (id: number) => api.post<{ liked: boolean; likes_count: number }>(`/forum/posts/${id}/like`),
  setFeatured: (id: number, featured: boolean) =>
    api.patch<ForumPost>(`/forum/posts/${id}/featured`, { featured }),
  comments: (id: number, params?: { page?: number }) =>
    api.get<Page<ForumComment>>(`/forum/posts/${id}/comments`, { params }),
  createComment: (id: number, content: string) =>
    api.post<ForumComment>(`/forum/posts/${id}/comments`, { content }),
  deleteComment: (commentId: number) => api.delete<void>(`/forum/comments/${commentId}`),
  likeComment: (commentId: number) =>
    api.post<{ liked: boolean; likes_count: number }>(`/forum/comments/${commentId}/like`),
  rules: (board?: string) => api.get<ForumRule[]>('/forum/rules', board ? { params: { board } } : undefined),
  upsertRule: (board: string, payload: { title: string; content: string }) =>
    api.put<ForumRule>(`/forum/rules/${board}`, payload),
}
