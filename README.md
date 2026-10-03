# SI BBS

AI 主题社区论坛。Wiki 知识库 + 软件发布及索引（GitHub 项目收录），三级权限（Admin / Moderator / User），软删除 + 回收站。

## 文档

- [架构设计](docs/architecture.md)
- [API 文档](docs/api.md)
- [部署指南](docs/deployment.md)
- [测试指南](docs/testing.md)
- [性能调优](docs/performance.md)

## 技术栈

- 后端：Rust (Edition 2024) / Axum 0.8 / SQLite (sqlx, WAL) / mimalloc
- 前端：Vue 3 (Composition API) / Vite / Pinia / Vue Router / Naive UI
- 部署：Nginx + Alpine Linux (1C1G, 300 并发)

## 快速开始

```bash
cd backend && cargo run
cd frontend && npm install && npm run dev
```

## 开发

```bash
cd backend && cargo test
cd frontend && npm run test:unit
cd frontend && npx playwright test
```

## License

MIT, Copyright (c) 2026 hemingtsai
