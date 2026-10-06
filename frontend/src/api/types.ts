/** Response shapes returned by the backend. Kept in sync with `backend/src/models`. */

export type Role = 'admin' | 'moderator' | 'user'
export type ProjectStatus = 'pending' | 'approved' | 'rejected'

export interface Page<T> {
  items: T[]
  total: number
  page: number
  per_page: number
}

export interface AuthTokens {
  access_token: string
  refresh_token: string
  role: Role
  user_id: number
  username: string
}

export interface Me {
  id: number
  username: string
  role: Role
  banned: boolean
}

export interface Project {
  id: number
  name: string
  github_url: string
  owner: string
  repo: string
  description: string | null
  readme_raw: string | null
  language: string | null
  stars: number
  forks: number
  license: string | null
  topics: string[]
  category: string
  status: ProjectStatus
  submitted_by: number
  reviewed_by: number | null
  review_note: string | null
  readme_fetched_at: string | null
  created_at: string
  updated_at: string
}

export interface RatingSummary {
  project_id: number
  average: number
  count: number
}

export interface RateAck extends RatingSummary {
  my_score: number
}

export interface Comment {
  id: number
  project_id: number
  user_id: number
  username: string
  content: string
  created_at: string
}

export interface WikiPage {
  id: number
  title: string
  slug: string
  category: string
  content: string
  status: 'draft' | 'published'
  author_id: number
  author_username: string | null
  created_at: string
  updated_at: string
}

export interface WikiCategory {
  category: string
  count: number
}

export interface TrashItem {
  kind: 'wiki' | 'project' | 'comment'
  id: number
  name: string | null
  deleted_at: string | null
  deleted_by: number | null
  deleted_by_username: string | null
}

export interface AdminUser {
  id: number
  username: string
  email: string
  role: Role
  banned: number
  created_at: string
}

export interface Stats {
  users: number
  users_banned: number
  projects: number
  projects_pending: number
  projects_approved: number
  wiki_published: number
  comments: number
  ratings: number
  trashed: number
}
export type ForumBoard = 'models' | 'tools' | 'life'

export interface BoardInfo {
  slug: string
  post_count: number
}

export interface ForumPost {
  id: number
  board: ForumBoard
  title: string
  content: string
  author_id: number
  author_username: string | null
  is_featured: number
  likes_count: number
  comments_count: number
  created_at: string
  updated_at: string
}

export interface ForumComment {
  id: number
  post_id: number
  author_id: number
  author_username: string | null
  content: string
  likes_count: number
  created_at: string
}

export interface ForumRule {
  board: string
  title: string
  content: string
  updated_by: number | null
  updated_at: string
}
