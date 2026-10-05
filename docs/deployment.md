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

> 没有 `sqlite3` CLI 的话，也可以临时去掉 `--release` 用 `cargo run`，直接在代码里
> 把第一个用户插入成 admin，或用 `docker exec` 进容器用 `sqlite3`。

### Docker Compose

```bash
JWT_SECRET=$(openssl rand -hex 32) docker compose -f deploy/docker-compose.yml up -d
```

`deploy/docker-compose.yml` 只是官方镜像的薄封装，按需修改端口和环境变量。

## 环境变量

| 变量 | 默认 | 说明 |
| --- | --- | --- |
| `PORT` | `3000` | 监听端口（当前代码硬编码 3000，注意） |
| `DATABASE_URL` | `sqlite:///data/si-bbs.db?mode=rwc` | SQLite 路径 |
| `JWT_SECRET` | `dev-secret-change-me` | **生产必改**，`openssl rand -hex 32` |
| `ACCESS_TTL_SECS` | `900` | access token 有效期 |
| `REFRESH_TTL_SECS` | `604800` | refresh token 有效期 |
| `GITHUB_TOKEN` | 空 | GitHub API 令牌，提高限速（可选但推荐） |
| `GITHUB_API_BASE` | `https://api.github.com` | 测试/代理时覆盖 |
| `STATIC_DIR` | (未设置则不对外服务静态文件) | 前端产物目录，镜像内为 `/app/static` |
| `RUST_LOG` | `si_bbs_backend=info,tower_http=info` | tracing 过滤 |

> 注意：`main.rs` 目前把监听地址写死为 `0.0.0.0:3000`。如果需要用 `PORT`，
> 把 `Config` 里读 `PORT` 再传给 `TcpListener::bind` 即可（一处改动）。

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

单进程已经足够，nginx 只是用来 TLS 终止或加更多限流时：

```nginx
# 参考 nginx/si-bbs.conf
server {
    listen 80;
    location / {
        proxy_pass http://127.0.0.1:3000;
        proxy_set_header Host $host;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
    }
}
```

前端构建时生成的 `.br`/`.gz` 预压缩文件已经由后端直接提供，nginx 开
`gzip_static on` 即可命中 `.gz`。

## 升级与备份

- **备份**：停掉写入或直接复制 `si-bbs.db-wal` 为空时的 `si-bbs.db` 和 `si-bbs.db-wal`
  即可（WAL 模式下最稳的方式是先 `PRAGMA wal_checkpoint(TRUNCATE)`）。
- **升级**：替换二进制镜像 → 重启容器。迁移由 sqlx 在启动时自动跑。
