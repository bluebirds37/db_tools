# db_tools

现代化 Tauri 多数据库管理工具，支持深色/浅色模式与全局 AI 智能体。

## 功能

- 连接管理：增删改查、测试连接
- 库表浏览：数据库 / 表 / 字段 / 索引 / 外键
- 数据 CRUD：分页查询、插入、编辑、删除
- SQL 工作台：执行查询与写操作
- AI 智能体：OpenAI / Anthropic 协议、工具调用、对话历史、长期记忆、上下文压缩
- 主题：深色 / 浅色

## 支持的数据库

| 类型 | 默认连接 |
|------|----------|
| MySQL | localhost:3307 root/root |
| PostgreSQL | localhost:15432 postgres/postgres |
| PgVector | localhost:15433 |
| Redis | localhost:16379 |
| MongoDB | localhost:27017 admin/tradingagents123 |
| Elasticsearch | localhost:9200 |
| SQLite | 本地文件 |
| Qdrant | localhost:6333 |

备用 Docker Compose：`C:\DevelopTools\docker\db_tools\docker-compose.yml`

## 开发

```bash
cd db_tools_app
npm install
npm run tauri dev
```

## 构建

```bash
npm run tauri build
```

## AI 设置

在右侧面板「设置」中配置：

- OpenAI 兼容：`https://api.openai.com/v1`（或任意兼容网关）
- Anthropic：`https://api.anthropic.com`

启用工具后，AI 可直接列出连接、查表、执行 SQL、增删改数据，并把关键偏好写入长期记忆。

## 数据目录

连接、会话、记忆保存在 `~/.db_tools/`
