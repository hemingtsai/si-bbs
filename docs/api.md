# API 参考

所有端点都接在同一个 Axum 服务下面（`backend/src/routes.rs`）。出错时统一返回：

```json
{ "error": "具体错误信息" }
```

认证方式：`Authorization: Bearer <access_token>`。`access_token` 有效期 15 分钟，
`refresh_token` 有效期 7 天（`/api/auth/refresh` 换新）。角色相关的端点不仅校验 token，
还会回读数据库里的当前角色和封禁状态，因此**改角色、封禁立即生效**，无需等待 token 过期。

权限矩阵：

| 角色 | 说明 |
| --- | --- |
| `user` | 注册、提交项目、评分、评论、写 wiki 草稿 |
| `moderator` | user 全部权限 + 审核队列、编辑/删除任何 wiki 页面、删除项目 |
| `admin` | moderator 全部权限 + 用户目录、改角色、封禁、彻底清除回收站 |

## 健康检查

| 方法 | 路径 | 说明 |
| --- | --- | --- |
| GET | `/api/health` | 返回 `ok` |

## 认证

| 方法 | 路径 | 请求体 | 成功 | 说明 |
| --- | --- | --- | --- | --- |
| POST | `/api/auth/register` | `{username, email, password}` | 201 `{role}` | 用户名 3–32 字符且只允许字母/数字/`_`/`-`；邮箱需含单个 `@` 与带点域名且 ≤254 字符；密码 6–128 **字符**。用户名或邮箱重复（**忽略大小写**）返回 409；超限返回 400；注册接口有进程级配额，超限 429 |
| POST | `/api/auth/login` | `{username, password}` | 200 `{access_token, refresh_token, role, user_id, username}` | 封禁账号返回 403；同一账号（忽略大小写）在 5 分钟内失败 8 次后返回 429 并带 `Retry-After`，登录成功即清零 |
| POST | `/api/auth/refresh` | `{refresh_token}` | 200 `{access_token, refresh_token}` | 会重新读取数据库角色；被封禁或被删返回 403/401 |
| GET | `/api/auth/me` | — | 200 `{id, username, role, banned}` | 角色以数据库为准 |

## 项目

| 方法 | 路径 | 权限 | 说明 |
| --- | --- | --- | --- |
| GET | `/api/projects?category=&q=&sort=stars|recent|name&page=&per_page=` | 公开 | 只列 `approved` 项目；`q` 搜索标题和描述 |
| POST | `/api/projects` | 登录 | `{github_url, category, description?}`，抓取 GitHub 元数据和 README 后进入 `pending`，唯一同 URL 返回 409 |
| GET | `/api/projects/mine` | 登录 | 当前用户的提交，任意状态 |
| GET | `/api/projects/review-queue` | mod+ | 待审核列表，按提交时间升序 |
| GET | `/api/projects/{id}` | 公开* | 详情；README 超过 24 小时会懒刷新，抓取失败回退旧缓存 |
| POST | `/api/projects/{id}/review` | mod+ | `{action: "approve"|"reject", note?}`，驳回时 `note` 必填 |
| DELETE | `/api/projects/{id}` | 提交者/mod+ | 软删除，进入回收站 |

\* `pending`/`rejected` 的项目只有提交者和管理员/版主可见，其他人得到 404。

### 评分

| 方法 | 路径 | 权限 | 说明 |
| --- | --- | --- | --- |
| POST | `/api/projects/{id}/rating` | 登录 | `{score: 1..=10, comment?}`，同一用户重复打分会覆盖 |
| GET | `/api/projects/{id}/rating/summary` | 公开 | `{project_id, average, count}`，平均值保留一位小数 |

### 评论

| 方法 | 路径 | 权限 | 说明 |
| --- | --- | --- | --- |
| GET | `/api/projects/{id}/comments?page=&per_page=` | 公开 | 按时间升序 |
| POST | `/api/projects/{id}/comments` | 登录 | `{content}`，1–5000 字 |
| DELETE | `/api/projects/{project_id}/comments/{comment_id}` | 作者/mod+ | 软删除 |

## Wiki

`content` 字段一律是 Markdown 源文，由前端渲染并消毒，**后端不返回 HTML**。

| 方法 | 路径 | 权限 | 说明 |
| --- | --- | --- | --- |
| GET | `/api/wiki?category=&q=&page=&per_page=` | 公开 | 只列 `published`，搜索标题和正文 |
| GET | `/api/wiki/categories` | 公开 | `[{category, count}]`，已发布页面的分类统计 |
| GET | `/api/wiki/mine` | 登录 | 当前用户的页面（草稿 + 已发布）；staff 可见所有 |
| GET | `/api/wiki/{slug}` | 公开* | 草稿仅作者和 staff 可见，其他人 404 |
| POST | `/api/wiki` | 登录 | `{title, category, content, status?}`，status 默认 `draft`；slug 自动生成且唯一 |
| PUT | `/api/wiki/page/{id}` | 作者/staff | 更新标题、分类、正文、状态；slug 保持不变 |
| DELETE | `/api/wiki/page/{id}` | 作者/staff | 软删除 |

## 回收站

`kind` 只接受 `wiki` / `project` / `comment`。

| 方法 | 路径 | 权限 | 说明 |
| --- | --- | --- | --- |
| GET | `/api/trash` | 登录 | 普通用户只见自己删的；mod+ 可见全部，附 `deleted_by_username` |
| POST | `/api/trash/{kind}/{id}/restore` | 删除者/staff | 恢复；唯一键仍被占用时理论上可得 409 |
| DELETE | `/api/trash/{kind}/{id}` | admin | 彻底删除，不可恢复 |

软删除的行仍然占着原来的 UNIQUE 键（如 `projects.github_url`、`wiki_pages.slug`），
所以同一 URL/slug 在恢复前不能被重新使用。

## 管理后台

| 方法 | 路径 | 说明 |
| --- | --- | --- |
| GET | `/api/admin/users?q=&role=&page=&per_page=` | 用户目录；`role` 传非法值返回 400 |
| PATCH | `/api/admin/users/{id}/role` | `{role}`，不能降级唯一的 admin，不能降级自己 |
| PATCH | `/api/admin/users/{id}/ban` | `{banned: true|false}`，不可封禁自己或唯一的 admin |
| GET | `/api/admin/stats` | `{users, users_banned, projects, projects_pending, projects_approved, wiki_published, comments, ratings, trashed}` |

## 错误码

| 码 | 含义 |
| --- | --- |
| 400 | 参数校验失败（URL 非法、分数越界、分类为空等） |
| 401 | 未登录或 token 无效；恢复的 token 对被删用户也是 401 |
| 403 | 已登录但角色不够、被封禁、或操作他人的资源 |
| 404 | 资源不存在，或对当前用户不可见（pending 项目、草稿） |
| 409 | 唯一冲突（用户名/邮箱忽略大小写、GitHub URL、slug） |
| 429 | 触发限流（当前只有 `/api/auth/login` 与 `/api/auth/register`），响应带 `Retry-After` 秒数 |
| 500 | 服务端错误（响应体只有通用文案，真实原因写进服务端日志） |
