# db_tools

基于 Tauri 的现代化多数据库管理桌面应用。

## 项目位置

`db_tools_app/`

## 快速开始

```powershell
cd D:\CodeSpaces\vibe\db_tools\db_tools_app
npm install
npm run tauri dev
```

## 已预置本地连接

| 名称 | 类型 | 地址 |
|------|------|------|
| 本地 MySQL | MySQL | localhost:3307 root/root |
| 本地 PostgreSQL | PostgreSQL | localhost:15432 postgres/postgres |
| 本地 PgVector | PgVector | localhost:15433 |
| 本地 Redis | Redis | localhost:16379 |
| 本地 MongoDB | MongoDB | localhost:27017 admin/tradingagents123 |
| 本地 Elasticsearch | ES | localhost:9200 |
| 本地 SQLite | SQLite | ~/.db_tools/local.db |
| 本地 Qdrant | Qdrant | localhost:6333 |

备用 Docker 编排：`C:\DevelopTools\docker\db_tools\docker-compose.yml`

## 能力

- 深色 / 浅色主题
- 连接增删改查与测试
- 库 / 表 / 字段 / 索引 / 外键浏览
- 分页数据 CRUD
- SQL 工作台
- 全局 AI 智能体（OpenAI / Anthropic 协议、工具调用、历史、长期记忆、上下文压缩）
