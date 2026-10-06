# 性能与并发

目标：一台 **Alpine 1C1G**（1 vCPU / 1 GiB）扛 300 并发请求，P95 响应时间
低于 250ms。

**证据状态**：下面是一次可复现的实测（脚本在 `deploy/loadtest.sh`，k6 场景支持
`VUS=300`），跑在**开发机**上，不是 1C1G 目标机——1C1G 上的数字仍未验证。
另外 `ConcurrencyLimitLayer(64)` 会把同时在处理的请求限制在 64，超出的请求排队，
因此"300 并发"在实现上表现为排队而不是 300 路真并行：这一点在 c=300 的 P99 上
看得很清楚。

## 实测基线

Apple Silicon（arm64）+ Docker Desktop，容器内为发布镜像；数据集是
**2000 个已上架项目**（`/api/projects?per_page=20` 因此每次都要 COUNT + 取 20 行）：

```bash
docker run -d -p 3001:3000 -e JWT_SECRET=$(openssl rand -hex 32) si-bbs
ab -n 2000 -c 64  http://localhost:3001/api/projects
ab -n 2000 -c 300 http://localhost:3001/api/projects
ab -n 1000 -c 64  http://localhost:3001/api/health
```

| 场景 | 吞吐 | mean | P50 | P95 | P99 | 失败 |
| --- | --- | --- | --- | --- | --- | --- |
| `GET /api/projects`，c=64 | 1896 req/s | 33.8ms | 32ms | **39ms** | 54ms | 0 |
| `GET /api/projects`，c=300 | 495 req/s | 606ms | 137ms | **151ms** | 1024ms | 0 |
| `GET /api/health`，c=64 | 8184 req/s | — | — | 11ms | — | 0 |

读出两件事：

1. c=64 时热路径很宽裕（P95 39ms，远低于 250ms 目标），吞吐也比旧文档记的
   `~491 req/s` 高得多——那是更早的构建与更小的数据集。
2. c=300 时 **P95 仍在目标内（151ms）**，但 P99 涨到 1s、均值 606ms：这正是
   64 槽并发上限造成的排队，请求没有失败，只是尾延迟变差。要改善尾延迟得提高
   上限或减少单请求工作量，而不是加机器。

注意 `ab` 不判定阈值（5xx 也会 exit 0），它只提供上面这些数字；要"失败即红"
请用 k6（`VUS=300 deploy/loadtest.sh http://localhost:3000`，阈值写在
`deploy/k6.js`）。

1C1G 目标机的数字**仍未验证**：小机器上应重跑同一组命令并把输出留档。

## 为什么够快

1. **单线程 Tokio**：`tokio::main(flavor = "current_thread")`。1 vCPU 机器上，
   调度器反而更简单，没有跨核同步开销。
2. **mimalloc**：musl 下 glibc malloc 碎片严重，mimalloc 显著降低小对象分配的
   内存占用。
3. **SQLite WAL + 读多写少**：WAL 允许读者并发；写入走单连接串行，BBS 写入极小。
   `PRAGMA foreign_keys=ON` 只有约束开销，没有锁竞争。
4. **每次请求一次主键查询换权限即时生效**：`require_auth` 除了验签，还会回读
   `users.role/banned`（封禁立即 403、角色以库为准），`require_role` 只是其上的
   白名单。这是有意用一次索引查询换掉"旧 token 权限过期"这个安全问题，
   在 SQLite 上可忽略；早期版本只在角色端点上查库，代价是权限撤销有 15 分钟窗口。
5. **Argon2 用 `spawn_blocking`**：哈希计算不阻塞 Tokio 线程，慢请求不波及健康检查。
6. **静态资源预压缩 + 大响应即时压缩**：静态文件由前端构建写出 `.br`/`.gz`，
   后端 `ServeDir` 显式开启 `precompressed_br()/precompressed_gzip()` 后直接按
   `Accept-Encoding` 送预压缩件，热路径上不做静态压缩；API 的 JSON 响应则由
   `CompressionLayer` 即时压缩，但只压 ≥1KB 的（`SizeAbove` 谓词），
   带 `Content-Encoding` 的静态响应会被它自动跳过，不会二次压缩。
   实测 `vendor-*.js` 175.6KB → Brotli 58.7KB，`index-*.js` 11.6KB → Brotli 3.7KB。
   字体不参与预压缩：WOFF2 本身已是 Brotli、WOFF 已是 zlib，再压出来的 `.br`/`.gz`
   比原文件还大（6.07MB woff2 → 6.08MB .br + 6.08MB .gz），白白进了镜像。
7. **前端手动分包**：`marked`/`highlight.js` 单独成块（192.7KB → Brotli 47.8KB），
   首屏拉 `index`（11.6KB → Brotli 3.7KB）+ `vendor`（175.6KB → Brotli 58.7KB）
   + 两份 CSS（110.8KB → Brotli 23.7KB）。

## 已知的瓶颈与对策

| 场景 | 表现 | 对策 |
| --- | --- | --- |
| 写操作被读挡住 | SQLite 写锁串行，大并发写会排队 | BBS 写操作极少；继续加就上 PostgreSQL，改 `sqlx::PgPool` |
| 刷新抖动 | 同一项目 24h 内只成功刷新一次；失败后 10 分钟内不再重试 | `readme_fetched_at`（成功时间）与 `readme_attempted_at`（尝试时间）分开，失败也会记录尝试。一次刷新最多打两次 GitHub（元数据 + README），都在失败退避的管辖内 |
| 项目元数据陈旧 | `stars`/`forks`/`topics` 曾只在提交时抓取，之后不再更新，"按星排序"会逐渐失真 | 已随 24h 懒刷新一起更新：刷新时先取一次 `GET /repos/{owner}/{repo}` 写回计数，再单独取 README；README 失败不影响计数更新 |
| GitHub API 限速 | 匿名 60 次/小时/IP | 配置 `GITHUB_TOKEN` 提到 5000 次/小时 |
| 首屏 JS 体积 | marked+highlight 很重 | 已用 `highlight.js/lib/common` 而非全量（省约 1MB），按路由懒加载详情页 |
| 并发压垮内存 | 瞬时大流量 | `ConcurrencyLimitLayer(64)` 限并发——注意它是**排队背压**，不是快速失败/503；要拒绝得另加 `LoadShedLayer` |

## 复现压测

```bash
# 用 oha（CI 友好）
deploy/loadtest.sh http://localhost:3001

# 或 k6（更细的指标）
BASE=http://localhost:3001 k6 run deploy/k6.js
```

`oha` 输出里看 `Requests/sec` 和 `P95`；`k6` 的 thresholds 已写在
`deploy/k6.js` 里（P95 < 250ms、错误率 < 1%），`VUS` 可覆盖（默认 64）。

## 镜像体积

发布镜像实测 **42.2MB**：Alpine 3.21 + 9.4MB musl 静态二进制 + 7.5MB 前端 `dist`。

体积上有过两处真实浪费，都已修掉并用 `docker images` 复测：

1. `dist` 曾是 19MB，其中约 12MB 是对字体做无效预压缩产生的 `.br`/`.gz`
   （woff2 本身已是 Brotli，再压出来的文件比原文件还大）。把 `woff2?` 排除出
   预压缩列表后 `dist` 降到 7.2MB。
2. 运行镜像里 `chown -R /data /app` 会把整个 `/app` 重写进新图层——9.16MB 的
   静态产物换来 17MB 的图层。改用 `COPY --chown` 之后该图层消失，
   **实测 70.8MB → 42.2MB**。（同时删掉了运行镜像里的 `migrations/` 目录：
   `sqlx::migrate!` 已在编译期把迁移嵌进二进制，留着只会让人误改一个没人读的目录；
   已在容器里复验"无该目录仍会跑完 14 个迁移"。）

如需进一步缩小：把 SQLite 换成 LiteFS 之类无需变更；前端只用最常用的
页面路由做预渲染也能再省首屏体积。
