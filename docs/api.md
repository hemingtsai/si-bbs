# API 参考

所有端点都接在同一个 Axum 服务下面（`backend/src/routes.rs`）。应用层的错误统一返回：

```json
{ "error": "具体错误信息" }
```

**例外**：提取器层的失败（畸形 JSON、字段类型不对、路径参数不是整数、请求体超过
2MB）由 axum 直接处理，返回的是纯文本 400/422/413，不带这个 JSON 结构；需要严格
契约的调用方要把这两种情况都考虑进去。

认证方式：`Authorization: Bearer <access_token>`。`access_token` 有效期 15 分钟，
`refresh_token` 有效期 7 天（`/api/auth/refresh` 换新）。**每个**需要登录的端点都会
回读数据库里的当前角色与封禁状态（不只是角色端点），因此改角色、封禁都立即生效，
无需等待 token 过期，代价是每个请求多一次主键查询。

权限矩阵：

| 角色 | 说明 |
| --- | --- |
| `user` | 注册、提交项目、评分、评论、写 wiki 草稿、发帖/回帖、点赞 |
| `moderator` | user 全部权限 + 审核队列、编辑/删除任何 wiki 页面、删除任何项目/评论/帖子、精选帖、维护板规、看到全站回收站 |
| `admin` | moderator 全部权限 + 用户目录、改角色、封禁、彻底清除回收站、站点统计 |


## 健康检查

| 方法 | 路径 | 说明 |
| --- | --- | --- |
| GET | `/api/health` | 返回 `ok` |

## 认证

| 方法 | 路径 | 请求体 | 成功 | 说明 |
| --- | --- | --- | --- | --- |
| POST | `/api/auth/register` | `{username, email, password}` | 201 `{role}` | 用户名 3–32 字符且只允许字母/数字/`_`/`-`；邮箱需含单个 `@` 与带点域名且 ≤254 字符；密码 6–128 **字符**。用户名或邮箱重复（**忽略大小写**）返回 409；超限返回 400；注册接口有进程级配额，超限 429 |
| POST | `/api/auth/login` | `{username, password}` | 200 `{access_token, refresh_token, role, user_id, username}` | 封禁账号返回 403；同一账号（忽略大小写）在 5 分钟内失败 8 次后返回 429 并带 `Retry-After`，登录成功即清零 |
| POST | `/api/auth/forgot` | `{email}` | 202 `{status}` | 申请重置链接。**无论邮箱是否存在都返回同一个 202 体**（否则就是账号存在性预言机），响应里也绝不含 token。按地址与进程双重限流，被限流时同样返回通用 202 |
| POST | `/api/auth/reset` | `{token, new_password}` | 200 `{status}` | 用链接里的 token 设新密码。token 一次性、30 分钟过期、请求新链接会作废旧的；成功后 `token_version` 自增，**所有既有会话失效**（不会自动登录）。token 无效/过期/已用一律 400，不区分原因 |
| POST | `/api/auth/refresh` | `{refresh_token}` | 200 `{access_token, refresh_token}` | 会重新读取数据库角色；被封禁或被删返回 403/401 |
| GET | `/api/auth/me` | — | 200 `{id, username, email, role, banned}` | 角色以数据库为准 |
| GET | `/api/auth/profile` | 登录 | 自己的资料：`{id, username, display_name, email, bio, avatar_url, role, banned, created_at}` |
| PATCH | `/api/auth/profile` | 登录 | `{display_name?, bio?, avatar_url?}`，均可选：**未传的字段保持原值，传空串表示清空**。昵称 ≤32 字符（允许空格与中文，禁止控制字符）、简介 ≤500 字符（允许换行）、头像必须是 `http(s)://` 绝对地址且 ≤500 字符（拒绝 `javascript:`/`data:`/相对路径） |
| POST | `/api/auth/email` | 登录 | `{password, new_email}` | 改自己的邮箱；需当前密码，格式校验同注册，**忽略大小写唯一**（冲突 409）；密码错误 401 并计入失败配额 |
| POST | `/api/auth/password` | 登录 | `{current_password, new_password}` | 改自己的密码。新密码 6–128 字符且必须与旧密码不同；**成功后 `users.token_version` 自增，此前签发的所有 access/refresh token 立即失效**（含被改密码者自己的），因此响应会返回一对新 token 供当前会话继续使用。当前密码错误返回 401，并计入与登录相同的失败配额（5 分钟 8 次后 429） |

公开内容里的作者名统一取 `COALESCE(users.display_name, users.username)`：设了昵称就显示昵称，否则回退到登录名。管理端用户目录、审计日志与回收站的 `deleted_by_username` 仍使用**真实登录名**（那些场景要的是"谁"，不是"显示成什么"）。前端判断"能否删这条评论"已改为比较 `user_id` 而不是名字。

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
| POST | `/api/projects/{id}/rating` | 登录 | `{score: 1..=10, comment?}`（`comment` ≤1000 字），同一用户重复打分会覆盖；不能给未上架项目打分 |
| GET | `/api/projects/{id}/rating/summary` | 公开 | `{project_id, average, count}`，平均值保留一位小数 |

### 评论

| 方法 | 路径 | 权限 | 说明 |
| --- | --- | --- | --- |
| GET | `/api/projects/{id}/comments?page=&per_page=` | 公开 | 按时间升序 |
| POST | `/api/projects/{id}/comments` | 登录 | `{content}`，1–5000 字 |
| DELETE | `/api/projects/{project_id}/comments/{comment_id}` | 作者/mod+ | 软删除 |

## 论坛

三个固定板块 `models` / `tools` / `life`。**发帖与回复都不经审核**，写完即可见；
帖子与回复都能点赞（切换式，「每用户每目标一行」由 `forum_likes` 主键保证），
点赞数与回复数在读取时现算，因此不会出现计数与行数不一致。

| 方法 | 路径 | 权限 | 说明 |
| --- | --- | --- | --- |
| GET | `/api/forum/boards` | 公开 | `[{slug, post_count}]`，三个板块及未删帖数 |
| GET | `/api/forum/posts?board=&q=&page=&per_page=` | 公开 | 只列未删帖；`q` 搜标题与正文；精选优先，其后按时间倒序 |
| POST | `/api/forum/posts` | 登录 | `{board, title, content}`，标题 ≤200 字、正文 ≤50000 字 |
| GET | `/api/forum/posts/{id}` | 公开 | 详情，含 `likes_count` / `comments_count` / `is_featured` |
| PATCH | `/api/forum/posts/{id}` | 作者/mod+ | `{board, title, content}` 整体更新 |
| DELETE | `/api/forum/posts/{id}` | 作者/mod+ | 软删除，进入回收站 |
| POST | `/api/forum/posts/{id}/like` | 登录 | 切换点赞，返回 `{liked, likes_count}` |
| PATCH | `/api/forum/posts/{id}/featured` | mod+ | `{featured: bool}`，精选帖排在列表最前 |
| GET | `/api/forum/posts/{id}/comments?page=&per_page=` | 公开 | 按时间升序 |
| POST | `/api/forum/posts/{id}/comments` | 登录 | `{content}`，≤50000 字，免审核 |
| DELETE | `/api/forum/comments/{id}` | 作者/mod+ | 软删除回复 |
| POST | `/api/forum/comments/{id}/like` | 登录 | 切换点赞 |
| GET | `/api/forum/rules?board=` | 公开 | 不带 `board` 返回全部板规；带 `board` 返回总站规 + 该板板规 |
| PUT | `/api/forum/rules/{board}` | mod+ | `{title, content}`，`board` 可为 `global`/`models`/`tools`/`life` |

## Wiki

`content` 字段一律是 Markdown 源文，由前端渲染并消毒，**后端不返回 HTML**。

| 方法 | 路径 | 权限 | 说明 |
| --- | --- | --- | --- |
| GET | `/api/wiki?category=&q=&page=&per_page=` | 公开 | 只列 `published`，搜索标题和正文 |
| GET | `/api/wiki/categories` | 公开 | `[{category, count}]`，已发布页面的分类统计 |
| GET | `/api/wiki/mine` | 登录 | 当前用户的页面（草稿 + 已发布）；staff 可见所有 |
| GET | `/api/wiki/{slug}` | 公开* | 草稿仅作者和 staff 可见，其他人 404。**改过 slug 的页面在旧 slug 上仍可访问**（按别名解析，返回体带当前 slug，客户端据此改写 URL） |
| POST | `/api/wiki` | 登录 | `{title, category, content, status?}`，status 默认 `draft`；slug 自动生成且唯一 |
| PUT | `/api/wiki/page/{id}` | 作者/staff | 更新标题、分类、正文、状态，可选 `slug`（改名后旧 slug 进别名表，仍可访问）、可选 `base_revision`、可选 `comment`。**带 `base_revision` 时若期间有人保存过会返回 409**，不再静默覆盖 |
| GET | `/api/wiki/page/{id}/revisions?page=&per_page=` | 公开* | 版本历史（倒序，仅元数据与正文长度，不含正文）。草稿仅作者/staff |
| GET | `/api/wiki/page/{id}/revisions/{no}` | 公开* | 单个版本（含正文） |
| POST | `/api/wiki/page/{id}/revert/{no}` | 作者/staff | `{base_revision?, comment?}`，把第 `no` 版作为**新版本**追加（不改写历史，因此可以再回滚回来） |
| DELETE | `/api/wiki/page/{id}` | 作者/staff | 软删除 |

### Wiki 版本与并发

- 每次保存都会写入一条 `wiki_revisions` 快照，`wiki_pages.revision` 是权威计数
  （创建即 1）。因此 `revision=N` 表示"第 N 次保存之后的状态"。
- 并发用 **compare-and-set**：`UPDATE … WHERE id = ?1 AND revision = ?2`。
  编辑器传 `base_revision` 时，落后于当前版本直接 409 并告知双方版本号；
  不传时用读取到的版本兜底，仍能挡住"读到写之间被别人改掉"的竞态。
- 回滚也是新增版本（注释默认 `revert to revision N`），历史永远线性可追。
- 回收站**彻底删除**页面时，`wiki_revisions` 与 `wiki_slug_aliases` 通过
  `ON DELETE CASCADE` 一并清掉（否则 purge 会被外键拒绝）。

## 回收站

`kind` 接受 `wiki` / `project` / `comment` / `forum_post` / `forum_comment`
（与 `trash_view` 的联合分支一一对应，有测试保证两者不漂移）。

| 方法 | 路径 | 权限 | 说明 |
| --- | --- | --- | --- |
| GET | `/api/trash` | 登录 | 普通用户只见自己删的；mod+ 可见全部，附 `deleted_by_username`。**不分页，返回全部** |
| POST | `/api/trash/{kind}/{id}/restore` | 删除者/staff | 恢复；唯一键仍被占用时返回 409 |
| DELETE | `/api/trash/{kind}/{id}` | admin | 彻底删除，不可恢复。同一事务内先删依赖它的子行（评论、评分、回复、点赞） |

软删除的行仍然占着原来的 UNIQUE 键（如 `projects.github_url`、`wiki_pages.slug`），
所以同一 URL/slug 在恢复前不能被重新使用。

## 管理后台

| 方法 | 路径 | 说明 |
| --- | --- | --- |
| GET | `/api/admin/users?q=&role=&page=&per_page=` | 用户目录；`role` 传非法值返回 400 |
| PATCH | `/api/admin/users/{id}/role` | `{role}`，不能降级唯一的 admin，不能降级自己 |
| PATCH | `/api/admin/users/{id}/ban` | `{banned: true|false}`，不可封禁自己或唯一的 admin |
| GET | `/api/admin/stats` | `{users, users_banned, projects, projects_pending, projects_approved, wiki_published, comments, ratings, trashed}` |

## 密码找回的投递方式

`POST /api/auth/forgot` 需要一个投递渠道，而本仓库**还没有邮件发送能力**（没有
SMTP 配置、也没有引入邮件依赖）。因此当前行为是：把形如
`{PUBLIC_BASE_URL}/reset?token=…` 的链接写入**服务端 WARN 日志**，由运维/管理员
转达；响应体里永远不含 token。这是有意的降级而不是假装发信：

- token 只存 SHA-256 摘要，库被读走也拿不到可用链接；一次性、30 分钟、单活跃。
- 日志里出现的是 user_id 与链接，管理员本来就能直接改库/改文件，所以没有提权。
- 要真正"自助"，需要接入邮件投递（例如 lettre + SMTP 环境变量），
  这是部署侧的下一步，见 `docs/deployment.md`。

## 举报

任何登录用户都可以举报**公开且未删除**的内容（软删的帖子、草稿 wiki、待审项目
都会被 404 掉，举报它们只会制造无用工单）。同一用户对同一目标只有一条记录，
重复举报返回 200 `already reported`。

| 方法 | 路径 | 权限 | 说明 |
| --- | --- | --- | --- |
| POST | `/api/reports` | 登录 | `{target_kind, target_id, reason}`；`target_kind` ∈ `forum_post`/`forum_comment`/`wiki`/`project`/`comment`，`reason` ≤500 字；每小时 20 条上限，超限 429 |
| GET | `/api/reports?status=&kind=&page=&per_page=` | mod+ | 队列，默认只列 `status=open`（`all` 可查全部），按时间正序（先到先处理）。返回 `target_title`（帖子标题 / 评论摘要）与 `target_deleted`，无需二次请求 |
| PATCH | `/api/reports/{id}` | mod+ | `{status: "resolved"\|"dismissed", note?}`；已处理的再改返回 409，非法值 400；动作写入审计日志 |
| GET | `/api/reports/mine` | 登录 | 我提交过的举报及处理结果 |

**闭环**：如果版主直接删除了被举报的内容，该目标的未处理举报会被自动置为
`resolved` 并附 `note="content removed"`，不会留在队列里。删除路径覆盖
论坛帖子/回复、wiki 页面、项目、项目评论。

## 订阅（RSS）

公开、无需认证——feed 阅读器不会带 `Authorization`。每个源最多 30 条，
正文只放 600 字摘要（wiki 正文可达 20 万字符，整篇塞进 feed 没有意义）。

| 路径 | 内容 |
| --- | --- |
| `GET /feed.xml` | 全站：论坛新帖 + Wiki 变更，按时间倒序混合 |
| `GET /forum/feed.xml[?board=models\|tools\|life]` | 论坛新帖，可按板块过滤 |
| `GET /wiki/feed.xml` | 已发布 Wiki 页面的最近变更 |
| `GET /projects/feed.xml` | 新收录（approved）的项目 |

要点：

- `Content-Type: application/rss+xml; charset=utf-8`，RSS 2.0，带
  `<atom:link rel="self">`。带 `?board=` 非法值返回 400。
- 链接是**绝对地址**：优先用 `PUBLIC_BASE_URL`（生产建议设成
  `https://sibbs.cn`），否则按 `X-Forwarded-Proto` + `Host` 推导，
  因此反向代理下也能给出正确链接。
- 输出前会做 XML 转义，并丢弃 XML 1.0 无法表示的控制字符
  （一个 `\u{1}` 就能让整个 feed 无法解析）。
- 只包含公开可见内容：软删除的帖子、草稿 wiki、待审项目都不会出现。
- SPA 的 `index.html` 里带了 `<link rel="alternate" type="application/rss+xml">`
  自动发现声明，侧栏也有 RSS 入口。
- **反向代理必须把这些路径转给后端**：它们不是磁盘上的文件。
  `deploy/Caddyfile` 与 `nginx/si-bbs.conf` 都已包含
  （Caddy 用 `@backend path /api/* /feed.xml …`，nginx 用一条正则 location）。

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
