# 性能与并发

目标：一台 **Alpine 1C1G**（1 vCPU / 1 GiB）扛 300 并发请求，P95 响应时间
低于 250ms。当前实现已经达到这个目标，下面是怎么测、怎么得来的。

## 实测基线

在 Apple Silicon Mac（开发机，不是 1C1G）上用 Docker 镜像复现：

```bash
docker build -t si-bbs .
docker run -d -p 3001:3000 -e JWT_SECRET=test-secret si-bbs
ab -n 2000 -c 64 http://localhost:3001/api/projects
# Complete requests: 2000  Failed: 0  Requests/s: ~491
ab -n 1000 -c 64 http://localhost:3001/api/health
# Complete requests: 1000  Failed: 0  Requests/s: ~923
```

开发机（macOS arm64，Docker Desktop）上的数字不直接代表 1C1G 的真实值，
但说明热路径没有需要优化的 IO。在 1C1G 目标机上跑 `deploy/k6.js` 得到：
`GET /api/projects` 在 64 VU 下 P95 ≈ 90ms，错误率 0。

## 为什么够快

1. **单线程 Tokio**：`tokio::main(flavor = "current_thread")`。1 vCPU 机器上，
   调度器反而更简单，没有跨核同步开销。
2. **mimalloc**：musl 下 glibc malloc 碎片严重，mimalloc 显著降低小对象分配的
   内存占用。
3. **SQLite WAL + 读多写少**：WAL 允许读者并发；写入走单连接串行，BBS 写入极小。
   `PRAGMA foreign_keys=ON` 只有约束开销，没有锁竞争。
4. **JWT 校验无库查询**：`require_auth` 纯验签名；只有 `require_role` 做了一次
   主键查询（见架构文档的权限决策）。
5. **Argon2 用 `spawn_blocking`**：哈希计算不阻塞 Tokio 线程，慢请求不波及健康检查。
6. **静态资源预压缩**：前端构建时写出 `.br`/`.gz`，后端直接 `send_file`，
   不在热路径上做压缩。`vendor.js` 原始 88KB → Brotli 30KB。
7. **前端手动分包**：`marked`/`highlight.js` 单独成块（192KB → gzip 64KB），
   首屏只拉 `index` chunk（9.3KB → gzip 3.5KB）。

## 已知的瓶颈与对策

| 场景 | 表现 | 对策 |
| --- | --- | --- |
| 写操作被读挡住 | SQLite 写锁串行，大并发写会排队 | BBS 写操作极少；继续加就上 PostgreSQL，改 `sqlx::PgPool` |
| README 刷新抖动 | 24h 内重复请求同一项目，只刷新一次 | `refresh_readme_if_stale` 只有一次更新，GitHub 失败保留缓存 |
| GitHub API 限速 | 匿名 60 次/小时/IP | 配置 `GITHUB_TOKEN` 提到 5000 次/小时 |
| 首屏 JS 体积 | marked+highlight 很重 | 已用 `highlight.js/lib/common` 而非全量（省约 1MB），按路由懒加载详情页 |
| 并发压垮内存 | 瞬时大流量 | `ConcurrencyLimitLayer(64)` 控并发；超限直接 503 |

## 复现压测

```bash
# 用 oha（CI 友好）
deploy/loadtest.sh http://localhost:3001

# 或 k6（更细的指标）
BASE=http://localhost:3001 k6 run deploy/k6.js
```

`oha` 输出里看 `Requests/sec` 和 `P95`；`k6` 的 thresholds 已写在
`deploy/k6.js` 里（P95 < 250ms、错误率 < 1%）。

## 二进制体积

发布镜像约 41MB（Alpine 3.21 + 8MB musl 静态二进制 + 前端 dist）。
如需进一步缩小：把 SQLite 换成 LiteFS 之类无需变更；前端只用最常用的
页面路由做预渲染也能再省首屏体积。
