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

测试文件与覆盖范围：

| 文件 | 覆盖 |
| --- | --- |
| `tests/auth_test.rs` | 注册 201、登录、错误密码 401、重复注册 409、未登录 401 |
| `tests/project_test.rs` | URL 解析、提交、去重 409、审核队列、README 24h 懒刷新与失败回退、token 透传 |
| `tests/rating_comment_test.rs` | 评分 upsert/越界 400、评论发布/空白 400、软删除后列表不可见 |
| `tests/wiki_test.rs` | slug 生成与去重、草稿可见性、作者/staff 权限、软删除 |
| `tests/trash_test.rs` | 软删除进回收站、恢复、权限、admin 才能彻底删除、唯一键占用 |
| `tests/admin_test.rs` | 用户列表分页/搜索、改角色即时生效、refresh 不复活旧角色、封禁 |
| `tests/db_test.rs` | 迁移能跑完、表结构齐全 |

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
npm run test:e2e:install   # 首次
npm run test:e2e
```

`playwright.config.ts` 会同时起后端（指向独立 SQLite 文件）和 Vite preview
服务器，`tests/e2e/*.e2e.ts` 里的用例通过真实 HTTP 打到后端。

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

- 每个集成测试必须在内存库上独立运行，禁止共享状态（`common::test_ctx()`
  每次都新建 pool）。
- GitHub 调用一律走 `GithubClient::new(&cfg)`，测试里通过 `GITHUB_API_BASE`
  指向 wiremock。**不要**在测试里打 `api.github.com`。
- wiremock 对同一 matcher **保留先注册的响应**，需要改变响应就换一个 mock
  服务器并用 `common::server_with()` 重建路由。
- 用户要有 admin/moderator 身份，先 `register` 再用
  `common::register_login_as_role()` 改库里的 `role`，最后用新 token。
