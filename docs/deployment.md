# 部署指南

SI BBS 是单一二进制应用：Axum 同时提供 `/api/*` 和前端静态文件。推荐通过
Docker 部署，一个容器就是完整应用，不需要额外的 Web 服务器。

## 前置要求

- Docker 20.10+（或直接用宿主机上的 Rust 1.98+ 和 Node 26+）
- SQLite 数据需要持久化（容器内 `/data`）

## 一键 Docker 部署

```bash
docker build -t si-bbs .
docker run -d --name si-bbs \
  -p 3000:3000 \
  -e JWT_SECRET="$(openssl rand -hex 32)" \
  -e GITHUB_TOKEN="ghp_xxx" \
  -v si-bbs-data:/data \
  si-bbs
```

打开 `http://localhost:3000` 即可。数据（SQLite 文件）落在 `si-bbs-data` volume。

开发环境（默认账号没有，先注册）：

```bash
curl -X POST http://localhost:3000/api/auth/register \
  -H 'Content-Type: application/json' \
  -d '{"username":"admin","email":"a@b.c","password":"secret123"}'
# 再把 role 改成 admin：
sqlite3 /path/to/si-bbs.db "UPDATE users SET role='admin' WHERE username='admin'"  # 或直接改数据库
```

> 运行镜像里**没有** `sqlite3` CLI（只有 busybox），所以改库要么在宿主机上对
> 挂载出来的库文件执行，要么临时起来一个带 sqlite3 的容器把卷挂进去。

### Docker Compose

```bash
JWT_SECRET=$(openssl rand -hex 32) docker compose -f deploy/docker-compose.yml up -d
```

`deploy/docker-compose.yml` 是本地构建（`build: ..`，仓库不发布镜像）的薄封装：
端口只绑到 `127.0.0.1`、带 healthcheck 与日志轮转，环境变量按需修改。

## 环境变量

| 变量 | 默认 | 说明 |
| --- | --- | --- |
| `BIND_ADDR` | `0.0.0.0:3000` | 监听地址。前面有 Caddy/nginx 时应设 `127.0.0.1:3000`，否则 API 直接暴露在公网（明文、无限流）。compose 里已经这么设了。**没有 `PORT` 这个变量。** |
| `DATABASE_URL` | `sqlite:///data/si-bbs.db?mode=rwc`（镜像内）；直接跑二进制时是 `sqlite://si-bbs.db?mode=rwc` | SQLite 路径 |
| `JWT_SECRET` | 开发构建：`dev-secret-change-me`；**release 构建：无默认值** | 生成用 `openssl rand -hex 32`。release 二进制在缺省、仍是开发值、或短于 32 字节时**直接拒绝启动**（`fatal: JWT_SECRET ...`，退出码 1），因为该默认值是公开字符串，任何人都能据此伪造管理员 token。 |
| `ACCESS_TTL_SECS` | `900` | access token 有效期 |
| `REFRESH_TTL_SECS` | `604800` | refresh token 有效期 |
| `GITHUB_TOKEN` | 空 | GitHub API 令牌，提高限速（可选但推荐） |
| `GITHUB_API_BASE` | `https://api.github.com` | 测试/代理时覆盖 |
| `STATIC_DIR` | (未设置则不对外服务静态文件) | 前端产物目录，镜像内为 `/app/static` |
| `RUST_LOG` | `si_bbs_backend=info,tower_http=info` | tracing 过滤 |

> 监听地址由 `BIND_ADDR` 决定（默认 `0.0.0.0:3000`），代码里没有 `PORT`。

### 密码找回目前依赖日志

还没有邮件投递能力：`POST /api/auth/forgot` 会把重置链接打到**服务端 WARN 日志**
（`password reset link (no mailer configured): https://…/reset?token=…`），
需要管理员转达给用户。响应体永远不含 token，token 也只在库里存 SHA-256 摘要、
一次性、30 分钟过期。要变成真正的自助流程，需要接入 SMTP（例如 lettre）并把
投递渠道从日志切过去。生产建议同时设置 `PUBLIC_BASE_URL`，否则链接里的
`http://127.0.0.1:3000` 对用户无意义。

### 探针、日志与停机

- **`GET /api/health` 会执行 `SELECT 1`**：只监听但连不上 SQLite 时返回 503
  `database unavailable`（而不是 200），否则编排层会持续把流量送进来。
  镜像里已声明 `HEALTHCHECK`（busybox `wget`，非 2xx 即失败），30s 间隔。
- **访问日志**：`TraceLayer` 把"请求完成"事件钉在 INFO，因此默认的
  `RUST_LOG=si_bbs_backend=info,tower_http=info` 就能看到
  `method=… path=… latency=… status=…`。tower-http 的默认级别是 DEBUG，
  默认过滤下等于没有访问日志。
- **优雅停机**：收到 SIGTERM/`Ctrl-C` 后先停止接收新连接、等在途请求结束，
  再执行 `PRAGMA wal_checkpoint(TRUNCATE)` 折叠 WAL，最后退出。
  `docker stop` / `rc-service si-bbs stop` 都会走到这条路径。
- **5xx 的定位**：响应体只有通用文案，真实原因（含 SQLite 驱动报错）写进
  服务端 ERROR 日志，例如 `tracing::error!(error = …)`。

## 用 Caddy 反向代理 + TLS（推荐，已用于生产）

服务器上的生产环境用 Caddy 2（`apk add caddy caddy-openrc`）：TLS 终止与证书自动
续签发生在 Caddy，后端只监听 `127.0.0.1:3000`，前端静态文件由 Caddy 直接从
`/srv/si-bbs/dist` 提供，`/api/*` 反代到后端。样例见 `deploy/Caddyfile`。

```bash
apk add caddy caddy-openrc
cp deploy/Caddyfile /etc/caddy/Caddyfile
caddy fmt --overwrite /etc/caddy/Caddyfile
rc-service caddy start && rc-update add caddy
```

后端二进制的启动参数：`BIND_ADDR=127.0.0.1:3000`（这样公网 3000 端口就关闭，只剩
22/80/443），`STATIC_DIR` 留空让 Caddy 负责静态文件。Nginx 也能起同样作用，
见下一节。

## 不用 Docker 直接部署（musl 静态）

```bash
# 1. 构建前端
cd frontend && npm ci && npm run build

# 2. 构建后端（在 alpine 或装了 musl target 的机器上）
cd ../backend
cargo build --release

# 3. 运行
STATIC_DIR=../frontend/dist \
DATABASE_URL="sqlite:////var/lib/si-bbs/si-bbs.db?mode=rwc" \
JWT_SECRET="$(openssl rand -hex 32)" \
./target/release/si-bbs-backend
```

二进制是纯静态 musl（reqwest 用 rustls，不依赖系统 OpenSSL），拷贝到
Debian/Ubuntu/Alpine/甚至 scratch 容器里都能直接跑。

## Nginx 反向代理（可选）

单进程已经足够，nginx 只值得在需要 TLS 终止或额外限流时引入。两种形态：

1. **全部交给二进制**：一个 `location / { proxy_pass http://127.0.0.1:3000; }`。
   安全头、`.br`/`.gz` 预压缩件、深链回退都由后端处理；
   此时 nginx 上的 `gzip_static on` **没有任何作用**（它只对 nginx 自己从磁盘
   读的文件生效）。
2. **nginx 自己服务前端**：那就是 `nginx/si-bbs.conf` 的写法——`root` 指向
   `dist`、`try_files $uri /index.html` 做深链回退、`/api/` 单独反代、
   `/assets/` 加 immutable 缓存与安全头，并显式声明
   `gzip_static on`（Brotli 需要额外编译 ngx_brotli 后开 `brotli_static`）。

   只有形态 2 才能命中预压缩件，也只有它能在 STATIC_DIR 为空时正确回退——
   形态 1 的纯反代写法会让 `/forum/1` 的刷新直接落到后端的 JSON 404。

**按 IP 限流只能在代理层做。** 应用看不到客户端 IP（没有 `ConnectInfo`，也不读
`X-Forwarded-For`），它的限流是按账号/按进程的。`nginx/si-bbs.conf` 因此给
`/api/auth/`（`10r/m` + burst 5）和 `/api/`（`20r/s` + burst 40）各配了一个
`limit_req_zone`，超限返回 429。Caddy 标准发行版没有限流指令，需要
`caddy-ratelimit` 插件。

两份代理配置都在容器里做过语法校验（`nginx -t`、`caddy validate`）；
nginx 那份还在容器里实测过：`/forum/1` 返回 `index.html`、
`/assets/*` 带 `Cache-Control: immutable` 与全部安全头、`/` 是 `no-cache`。

## 升级与备份

- **备份**：先 `PRAGMA wal_checkpoint(TRUNCATE)`（或正常停服，停机流程会自动
  checkpoint），再复制 `si-bbs.db`。
- **升级**：替换二进制/镜像 → 重启。迁移由 sqlx 在启动时自动跑。
