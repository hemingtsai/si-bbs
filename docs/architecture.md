# 架构设计

## 总览

```
┌────────────┐   HTTP    ┌──────────────────────────────────────┐
│  浏览器    │ ────────▶ │  Axum 服务（单一二进制）             │
│  Vue 3 SPA │ ◀──────── │                                      │
└────────────┘  静态文件  │  routes → middleware → handlers      │
                          │     ↓        ↓        ↓              │
                          │  AppState { pool, cfg }              │
                          │     ↓                                │
                          │  SQLite (WAL)  ·  services::github   │
                          │  services::auth (Argon2 / jsonwebtoken)
                          └──────────────────────────────────────┘
```

- **单一二进制同时提供 API 和静态资源**：`STATIC_DIR` 指向 Vite 构建产物时，
  未匹配的路径回退到 `index.html`（客户端路由），`/api/*` 的未知路径仍返回 JSON 404。
- **SQLite** 用 WAL 模式，`PRAGMA foreign_keys = ON`，迁移由 sqlx 管理。
  并发写入在 SQLite 下是串行化的，但 BBS 的写入很小，读远多于写，单文件换来零运维。
- **mimalloc** 作为全局分配器，在 Alpine musl 目标上显著降低碎片。
- **tower::limit::ConcurrencyLimitLayer(64)** 限制同时在处理的请求数；
  超过即快速失败，避免 1C1G 小机器被打爆。

## 模块布局（`backend/src`）

| 模块 | 职责 |
| --- | --- |
| `config.rs` | 环境变量 → `Config`（DB、JWT 密钥、GitHub token、静态目录路径） |
| `db.rs` | 连接池、WAL、外键、跑迁移 |
| `error.rs` | 统一错误类型，输出 `{ "error": ... }` JSON |
| `models/` | `sqlx::FromRow` 结构 + 对外 DTO（`ProjectOut`、`Page<T>`…） |
| `services/auth.rs` | Argon2 哈希（`spawn_blocking`）、JWT 签发/校验 |
| `services/github.rs` | GitHub REST 客户端（`GITHUB_API_BASE` 可覆盖以便测试）、README base64 解码 |
| `middleware/auth.rs` | `require_auth`（只验 token）、`require_role`（回读数据库角色与封禁状态） |
| `handlers/` | 按领域分文件：`auth`、`project`、`rating`、`comment`、`wiki`、`trash`、`admin` |
| `routes.rs` | `AppState`、路由表、并发上限、SPA 静态文件回退 |

前端（`frontend/src`）：

| 路径 | 职责 |
| --- | --- |
| `api/axios.ts` | 拦截器：自动带 token、401 时共享一次 refresh 并重试 |
| `api/index.ts` | 各端点的类型化封装 |
| `api/types.ts` | 与后端 DTO 对齐的 TypeScript 类型 |
| `stores/auth.ts` | Pinia 登录态（持久化到 localStorage） |
| `lib/markdown.ts` | `marked` 渲染 + `highlight.js/lib/common` 高亮 + DOMPurify 消毒 |
| `views/` | 列表/详情/编辑/管理页面 |

## 关键决策

**1. Markdown 渲染放前端，后端只存原文。**
README 和 wiki 页面直接存 Markdown 源文，后端永不生成 HTML。前端用 `marked`
渲染，`highlight.js/lib/common` 做代码高亮，再用 DOMPurify 消毒后插入 DOM。
这样后端不暴露任何 XSS 面，也避免了存储两份内容（原文和 HTML）不一致。

**2. README 懒刷新，不定时同步。**
历史版本曾对所有已收项目定时刷 GitHub，在小机器上是纯浪费。现在只在
`GET /api/projects/{id}` 时检查 `readme_fetched_at`，超过 24 小时才重新抓取。
两个时间戳分工不同：`readme_fetched_at` 只由**成功**的抓取推进（它回答"这份
文档有多旧"），`readme_attempted_at` 记录**每一次尝试**，失败后 10 分钟内不再
重试同一个项目——否则 GitHub 宕机期间每个详情请求都要等一次上游超时。
"GitHub 宕机"与"仓库没有 README"仍然区分处理：前者保留缓存，后者清空缓存。

**3. 权限判断以数据库为准。**
`require_role` 先验 JWT，再查一次 `users.role/banned`。这样隐式地把两个安全
问题（旧 token 权限过期、封禁用户仍能用到 token 过期）都解决了。代价是每个
受保护端点多一次主键查询，对 SQLite 可以忽略。

**4. refresh token 不继承旧角色。**
`/api/auth/refresh` 会重新读取目标用户的当前角色与封禁状态再签发新 token。
早期的实现直接把 refresh token 里的 role 抄进新 access token，会导致"已降级的
管理员永远能续期出管理员 token"。

**5. 软删除 + trash_view。**
三张表（`wiki_pages`、`projects`、`comments`）都有 `deleted_at/deleted_by`，
`trash_view` 是一个 UNION 视图，`/api/trash` 只读它。恢复只清空被删标记；
彻底删除由 admin 专属端点做。软删除的行仍占着原唯一键，所以 URL/slug
在恢复前不会被占用——这避免了"恢复时唯一冲突"的歧义。

**6. 单写者 SQLite + 并发上限。**
对的：读多写少、单机部署。`ConcurrencyLimitLayer(64)` 配合 `tokio::main(current_thread)`，
1C1G 上不会因线程数暴涨而崩。

## 数据库 Schema（见 `backend/migrations/`）

```
users       id, username*, email*, password_hash, role, banned, created_at, updated_at
wiki_pages  id, title, slug*, category, content, status, author_id→users,
            deleted_at, deleted_by→users, created_at, updated_at
projects    id, name, github_url*, owner, repo, description, readme_raw,
            language, stars, forks, license, topics, category, status,
            submitted_by→users, reviewed_by→users, review_note,
            readme_fetched_at, deleted_at, deleted_by, created_at, updated_at
ratings     id, project_id→projects, user_id→users, score (1..10), comment,
            UNIQUE(project_id, user_id), created_at, updated_at
comments    id, project_id→projects, user_id→users, content, status,
            deleted_at, deleted_by, created_at
trash_view  kind, id, name, deleted_at, deleted_by  （视图，不落盘）
```

`*` 表示 UNIQUE 约束。外键在 `PRAGMA foreign_keys=ON` 下生效。
