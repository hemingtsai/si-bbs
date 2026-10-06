# SI BBS

[![CI](https://github.com/hemingtsai/si-bbs/actions/workflows/ci.yml/badge.svg)](https://github.com/hemingtsai/si-bbs/actions/workflows/ci.yml)

AI 主题社区论坛。Wiki 知识库 + 软件发布及索引（GitHub 项目收录），三级权限
（Admin / Moderator / User），软删除 + 回收站，单一二进制部署。

## 文档

- [架构设计](docs/architecture.md)
- [API 文档](docs/api.md)
- [部署指南](docs/deployment.md)
- [测试指南](docs/testing.md)
- [性能与并发](docs/performance.md)

## 技术栈

- 后端：Rust (Edition 2024) / Axum 0.8 / SQLite (sqlx, WAL) / mimalloc / rustls
- 前端：Vue 3 (Composition API) / Vite / Pinia / Vue Router / marked + DOMPurify
- 部署：Docker (Alpine 1C1G, 300 并发) 或 musl 静态二进制

## 快速开始

```bash
# Docker（一条命令起完整应用）
docker build -t si-bbs .
docker run -d -p 3000:3000 -e JWT_SECRET=$(openssl rand -hex 32) si-bbs

# 或者分别启动开发服务器
cd backend && cargo run
cd frontend && npm install && npm run dev
```

## 测试

```bash
cd backend && cargo test          # 81 个后端测试
cd frontend && npm run test:unit  # 17 个前端单测
cd frontend && npm run test:e2e   # Playwright 端到端
```

## 项目结构

```
backend/    Rust 后端（lib + thin main），cargo test 覆盖全部
frontend/   Vue 3 + Vite + Pinia，npm run check 跑完整校验
docs/       技术文档（中英混排，API 用表）
deploy/     docker-compose.yml, k6.js, loadtest.sh
nginx/      可选的反向代理样例
```

## License

MIT, Copyright (c) 2026 hemingtsai
