# 测试指南

四层测试，从下往上覆盖的东西和运行命令不同：

## 1. 后端单元/集成测试（Rust）

```bash
cd backend
cargo test                    # 所有测试
cargo test --test auth_test   # 只跑认证
cargo test --test project_test
```

集成测试不经过网络，而是直接把 `create_router()` 交给 `axum-test::TestServer`。
SQLite 用 `:memory:`，每个测试一份库。GitHub 请求通过 `GITHUB_API_BASE`
指向 `wiremock` 起的 mock 服务器，不打真实 GitHub。

测试文件与覆盖范围（当前共 138 条：`cargo test` 逐二进制计数）：

| 文件 | 覆盖 |
| --- | --- |
| `tests/auth_test.rs` | 注册 201、登录、错误密码 401、重复注册 409、未登录 401；大小写重复 409、用户名/邮箱/密码边界 400、登录失败限流 429 与 `Retry-After`、成功登录清零配额 |
| `tests/project_test.rs` | URL 解析、提交、去重 409、审核队列、README 24h 懒刷新与失败回退、token 透传 |
| `tests/rating_comment_test.rs` | 评分 upsert/越界 400、评论发布/空白 400、软删除后列表不可见 |
| `tests/wiki_test.rs` | slug 生成与去重、草稿可见性、作者/staff 权限、软删除 |
| `tests/forum_test.rs` | 板块、免审核发帖/回帖、编辑删除权限、精选排序、板规；并发点赞计数一致性、回复计数派生 |
| `tests/trash_test.rs` | 软删除进回收站、恢复、权限、admin 才能彻底删除、唯一键占用；有评论/评分/回复时的级联彻底删除 |
| `tests/admin_test.rs` | 用户列表分页/搜索、改角色即时生效、refresh 不复活旧角色、封禁、统计计数 |
| `tests/db_test.rs` | 迁移能跑完、表结构齐全、计数列已删除、回收站视图聚合五种 kind、测试库强制外键 |
| `tests/static_files_test.rs` | SPA 深链回退、`/assets/*` immutable 缓存、安全头、JSON 404、预压缩件按 `Accept-Encoding` 送达 |
| `tests/compression_test.rs` | 大 JSON 响应即时压缩并带 `Vary`、小响应不压、`identity` 客户端不压 |
| `tests/health_test.rs` | `/api/health` 正常 200、数据库不可用时 503 |

## 2. 前端单元测试（Vitest）

```bash
cd frontend
npm run test:unit
```

覆盖：`auth` Pinia store（登录态、登出、stale role 覆盖）、Markdown 渲染
（XSS 过滤、代码高亮、未知语言转义）、DOMPurify 配置。

## 3. 前端端到端（Playwright）

```bash
cd frontend
npm run test:e2e:install   # 首次：下载 chromium（本机沙箱可能不允许）
npm run test:e2e
```

`playwright.config.ts` 会同时起后端和 Vite preview 服务器，用例在
`frontend/e2e/*.e2e.ts`，通过真实 HTTP 打到后端。

- **每次运行前重置数据库**：后端启动命令里先跑 `e2e/support/reset-db.mjs`
  删掉 `.playwright/`（已 gitignore）再启动，所以套件可重复执行、不会把测试
  用户和帖子写进开发库。重置**不能**写在 `playwright.config.ts` 顶层——Playwright
  会在每个 worker 进程里重新求值该模块，那样会把运行中的后端正在用的库删掉，
  所有请求立刻开始 500。
- **账号名带 `Date.now()` 后缀**，断言用相对增量而不是全站总数。
- 端口 3000/5173 必须先空闲：配置里 `reuseExistingServer: false`，因为开发实例
  用的是另一个数据库，静默复用它等于往开发者自己的数据里写测试数据。
- 需要 moderator/admin 的用例用 `e2e/support/db.ts` 的 `promoteUser()` 直接改库
  （与文档里"建第一个管理员"的方式一致），并且**先登录再提权**——权限必须来自
  数据库，这也顺带覆盖了"改角色立即生效"。
- 目前只覆盖 HTTP 接口层，**还没有浏览器级 UI 流程**：本仓库开发环境无法安装
  chromium（沙箱禁止写 `~/Library/Caches/ms-playwright`），UI 用例需要先
  `npm run test:e2e:install` 后再补。

## 4. 冒烟测试（curl）

```bash
STATIC_DIR=../frontend/dist DATABASE_URL="sqlite:////tmp/smoke.db?mode=rwc" \
cargo run
curl http://localhost:3000/api/health            # ok
curl http://localhost:3000/                      # index.html
curl http://localhost:3000/projects              # index.html（客户端路由回退）
curl http://localhost:3000/api/not-exist         # {"error":"not found"}
```

## 约定

- CI（`.github/workflows/ci.yml`）跑三件事，与本地命令一致：
  `cargo clippy --all-targets -- -D warnings` + `cargo test`；
  `npm run check`（typecheck + 单测 + build）；`npm run test:e2e`
  （CI 里会 `npx playwright install --with-deps chromium`，所以浏览器级用例
  在 CI 上是真的会跑的）。
- 每个集成测试必须在内存库上独立运行，禁止共享状态（`common::test_ctx()`
  每次都新建 pool）。
- GitHub 调用一律走 `GithubClient::new(&cfg)`，测试里通过 `GITHUB_API_BASE`
  指向 wiremock。**不要**在测试里打 `api.github.com`。
- wiremock 对同一 matcher **保留先注册的响应**，需要改变响应就换一个 mock
  服务器并用 `common::server_with()` 重建路由。
- 用户要有 admin/moderator 身份，先 `register` 再用
  `common::register_login_as_role()` 改库里的 `role`，最后用新 token。
