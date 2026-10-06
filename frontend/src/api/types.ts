/** Response shapes returned by the backend. Kept in sync with `backend/src/models`. */

export type Role = 'admin' | 'moderator' | 'user'
export type ProjectStatus = 'pending' | 'approved' | 'rejected'
export type ForumBoard = 'models' | 'tools' | 'life'

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
  /// Present when the endpoint that minted the pair also knows the profile (a
  /// password change returns them, so the shell can update without a second call).
  display_name?: string | null
  avatar_url?: string | null
}

export interface Me {
  id: number
  username: string
  /// What readers see instead of the login name, when set.
  display_name: string | null
  email: string
  bio: string | null
  avatar_url: string | null
  role: Role
  banned: boolean
  created_at: string
}

export interface ProfileInput {
  display_name?: string
  bio?: string
  avatar_url?: string
}

export interface AdminReport {
  id: number
  reporter_id: number
  reporter_username: string | null
  target_kind: 'forum_post' | 'forum_comment' | 'wiki' | 'project' | 'comment'
  target_id: number
  reason: string
  status: 'open' | 'resolved' | 'dismissed'
  handled_by: number | null
  handled_by_username: string | null
  handled_at: string | null
  note: string | null
  /// Title of the reported content, and whether it has since been deleted.
  target_title: string | null
  target_deleted: number
  created_at: string
}

export interface SearchHit {
  kind: 'wiki' | 'forum' | 'project'
  id: number
  title: string
  /// Only wiki results have one; the others are addressed by id.
  slug: string | null
  excerpt: string
  updated_at: string
}

export interface AuditEntry {
  id: number
  actor_id: number
  actor_username: string | null
  action: string
  target_kind: string
  target_id: number | null
  detail: string | null
  created_at: string
}

export interface MyReport {
  id: number
  target_kind: 'forum_post' | 'forum_comment' | 'wiki' | 'project' | 'comment'
  target_id: number
  reason: string
  status: 'open' | 'resolved' | 'dismissed'
  note: string | null
  target_title: string | null
  created_at: string
  handled_at: string | null
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
  readme_attempted_at: string | null
  created_at: string
  updated_at: string
}

/**
 * List payloads deliberately omit the bodies (a page of 20 wiki pages or project
 * READMEs could be megabytes) and carry a short `excerpt` instead. Detail views use
 * the full types below.
 */
export interface ProjectSummary
  extends Omit<Project, 'readme_raw' | 'readme_fetched_at' | 'readme_attempted_at'> {
  excerpt?: string
}

export interface WikiPageSummary {
  id: number
  title: string
  slug: string
  category: string
  status: 'draft' | 'published'
  author_id: number
  author_username: string | null
  revision: number
  excerpt?: string
  created_at: string
  updated_at: string
}

export interface ForumPostSummary {
  id: number
  board: ForumBoard
  title: string
  author_id: number
  author_username: string | null
  is_featured: number
  likes_count: number
  comments_count: number
  excerpt?: string
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

export interface WikiRevision {
  revision_no: number
  title: string
  slug: string
  category: string
  status: 'draft' | 'published'
  author_id: number
  author_username: string | null
  comment: string | null
  created_at: string
  /// Character count, not bytes: the list deliberately ships no bodies.
  content_chars: number
}

export interface WikiRevisionDetail extends Omit<WikiRevision, 'content_chars'> {
  content: string
}

export interface DiffLine {
  kind: 'same' | 'add' | 'remove'
  text: string
  old_no: number | null
  new_no: number | null
}

export interface WikiDiff {
  from: number
  to: number
  lines: DiffLine[]
  /// True when the change was too large for an exact diff and was reported as a
  /// wholesale replacement instead.
  coarse: boolean
  added: number
  removed: number
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
  revision: number
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
