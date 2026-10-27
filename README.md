# AgentDB

[**中文**](./README.md) | [English](./README.en.md)

> **Agent for Any Database** —— 让 agent 管理所有数据库。

基于 Tauri 2 + React 19 的多数据库管理桌面应用，支持深色 / 浅色主题与内置 AI 智能体。

## 界面总览

![界面总览](docs/images/01-overview.png)

首次打开的三块区域：左侧是连接列表（可展开到库 / 表，顶部能搜索连接名），顶栏是四个跨库工具（数据库同步 / 数据库迁移 / 数据导出 / 数据库备份，不满足前置条件时置灰并说明缺什么），右侧是 AI 智能体面板（模型切换、历史对话、快捷指令）。中间在没有选中对象时给出下一步提示。

## 文档

按主题拆分的详细文档在 [`docs/`](./docs/README.md)，写作与命名规范也写在那里：

| 文档 | 内容 |
| --- | --- |
| [01 快速开始](docs/guide/01-quick-start.md) | 安装、开发运行、界面分区、快捷键、日志位置 |
| [02 连接管理](docs/guide/02-connections.md) | 分类选择器新建连接、测试、建连超时与不阻塞 |
| [03 浏览与数据](docs/guide/03-browse-and-data.md) | 库表树、分页数据网格、筛选与排序、增删改与写操作回显 |
| [04 结构设计](docs/guide/04-structure-design.md) | 数据库设计器、可视化建表、字段 / 索引 / 外键 |
| [05 非关系引擎](docs/guide/05-non-sql-engines.md) | Redis 命令、ES / Qdrant REST、Mongo 查询与常用操作 |
| [06 同步与迁移](docs/guide/06-sync-and-migration.md) | 跨引擎同步、MySQL↔PG 迁移、逐表结果报告 |
| [07 导出与备份](docs/guide/07-export-mask-backup.md) | CSV / Excel 导出、列级脱敏、SQL 备份 |
| [08 AI 智能体](docs/guide/08-ai-agent.md) | 多模型配置与默认、会话取数范围、图片提问、工具调用与记忆 |
| [09 外观与字体](docs/guide/09-appearance.md) | 主题、全局字体设置、动效与状态反馈 |

## 开发

一键脚本会先体检环境，再决定装依赖 / 启动 / 打包：

| 平台 | 一键启动（dev） | 一键打包 | 只体检 |
| --- | --- | --- | --- |
| Windows | 双击 `scripts\dev.bat`，或 `powershell -File scripts\agentdb.ps1 dev` | 双击 `scripts\build.bat` | `scripts\agentdb.ps1 check` |
| Linux | `./scripts/agentdb.sh dev` | `./scripts/agentdb.sh build` | `./scripts/agentdb.sh check` |

体检覆盖：node 18+ / npm / cargo / rustc、WebView2 运行时（Windows）、webkit2gtk 等系统库（Linux，缺了会给出可复制的 apt 命令，或加 `--with-deps` 让它直接装）、tauri 图标是否齐全（缺图标打包必失败）、`node_modules` 与 44100 端口占用。

常用开关：Windows `-Clean`（先 `cargo clean -p agentdb_app`，坏产物时用）、`-NoInstall`、`-Proxy http://127.0.0.1:7897`；Linux 对应 `--clean` / `--no-install` / `--proxy URL` / `--bundles "deb appimage"`。

**端口被占用会自动换**：开发端口默认 44100，若被占用（或被 Hyper-V / WSL 整段保留）会自动挑一个空闲端口，并让 vite 与 tauri 用同一个，不用手改配置。

手动命令照常可用：

```bash
npm install
npm run tauri:dev      # 开发（自动选空闲端口）
npm run tauri dev      # 开发（固定 44100，端口被占会直接报错）
npm run tauri build    # 打包（Windows 出 NSIS）
```

前端单独构建：`npm run build`（`tsc && vite build`）。

## 打包与分发

Tauri **不能跨平台编译**：Windows 安装包必须在 Windows 上打，macOS 必须在 macOS 上打，Linux 必须在 Linux（本机可用 Docker/WSL）里打。四个平台的产物与 GitHub Release 由 `.github/workflows/build.yml` 一次完成：

| 平台 | 构建 | 产物 |
| --- | --- | --- |
| Windows | `windows-latest` | NSIS 安装包 `AgentDB_<版本>_x64-setup.exe` |
| macOS | `macos-latest`，`--target universal-apple-darwin` | 通用二进制 `.dmg`（Apple Silicon + Intel） |
| Ubuntu / Debian | `ubuntu-22.04` | `.deb` |
| 通用 Linux | `ubuntu-22.04` | `.AppImage` |

发布方式：打标签并推送，CI 构建完自动创建同名 Release 并挂上这四个文件（Release 已存在则覆盖更新）。

```bash
git tag v0.1.0
git push origin main v0.1.0
```

在 Actions 页面手动 `Run workflow`（不带标签）只会产出可下载的 Artifacts，不发 Release。

本机手动打包：

```bash
npx tauri build --bundles nsis                          # Windows（离线只需 NSIS；MSI/WiX 要联网下载）
npx tauri build --bundles dmg,app                       # macOS
npx tauri build --bundles deb,appimage                  # Linux（先装 libwebkit2gtk-4.1-dev 等依赖，见 workflow）
```

产物位置：`src-tauri/target/release/bundle/<类型>/`。


> 当前配置**没有做代码签名 / 公证**：Windows 首次运行会有 SmartScreen 提示，macOS 需要在「系统设置 → 隐私与安全性」里允许或 `xattr -dr com.apple.quarantine`，Linux 的 AppImage 需要先 `chmod +x`。要正式分发再补签名变量即可。


## 目录结构

```
.
├── .github/workflows/      # 三平台打包 CI（build.yml）
├── index.html              # 前端入口
├── package.json            # 前端依赖与 tauri CLI 脚本
├── vite.config.ts          # vite + tailwind v4
├── tsconfig.json
├── public/                 # 静态资源
├── docs/screenshots/       # 界面截图（较早版本的存档）
├── src/                    # React 前端
│   ├── components/         # 侧边栏、数据网格、设计器、对话框、可筛选下拉等
│   ├── services/api.ts     # 与 Rust 命令一一对应的 invoke 封装
│   ├── stores/appStore.ts  # zustand：主题 / 字体 / 连接 / 选中对象
│   └── styles/global.css   # 设计变量与动效
└── src-tauri/              # Rust 后端
    ├── Cargo.toml
    ├── tauri.conf.json     # devUrl / frontendDist / bundle
    ├── capabilities/       # 权限清单
    ├── icons/
    ├── examples/           # 驱动层手工验证脚本：cargo run --example xxx
    └── src/
        ├── commands.rs     # #[tauri::command] 入口
        ├── db/             # 各引擎驱动 + DbSession trait
        ├── transfer.rs     # 同步 / 迁移 / 导出 / 备份 / 脱敏
        ├── ai/             # OpenAI・Anthropic 协议、工具调用、记忆
        ├── models.rs       # 跨层数据结构
        └── store.rs        # 本机配置持久化
```

## 支持的数据库

MySQL、PostgreSQL、PgVector、SQLite、Redis、MongoDB、Elasticsearch、Qdrant。
连接全部由你自己在「新建连接」里添加，切换类型时会带上该引擎的标准默认端口。

## 能力

- **连接**：分类选择器（关系型 / 文档型 / 键值 / 搜索 / 向量）、新建与编辑、测试连接、最近使用
- **浏览与结构**：库 / 表 / 字段 / 索引 / 外键；数据库设计器（字符集、注释、PG 扩展）、可视化建表、字段与索引外键的增删改；结构页可让大模型补全表 / 字段注释并建议索引（预览勾选后才写库），一键复制建表语句
- **数据**：分页查询、按字段拼筛选条件（并且 / 或者、包含 / 介于 / 在列表中等，按字段类型给运算符）与排序，点表头即可升降序切换；表头可展开第二层显示字段描述（默认折叠，过长省略、悬停看全文）；插入、编辑、删除；表节点右键可导入 CSV / Excel、清空数据、删除表（破坏性操作都要红色危险样式的二次确认，并回显后端原话）
- **多入口**：SQL 工作台；Redis 走命令、ES / Qdrant 走 REST（`METHOD path [json]`）、Mongo 走查询文档，均附「常用操作」可点选清单
- **同步与迁移**：整库 / 单表同步（可跨引擎，落到 Redis 键或 Mongo 文档）、MySQL→PostgreSQL 迁移（类型归一、索引、自增序列回填），执行后给出逐表结果报告
- **导出与备份**：CSV / Excel 导出，支持列级脱敏（掩码、保留前后位、邮箱 / 手机 / 身份证 / 银行卡 / 哈希 / 置空 / 替换）；SQL 转储备份
- **行数上限**：导出 / 备份 / 同步 / 迁移统一语义，填 `0` 或 `-1` 表示全部行，逐页取数不整表驻留
- **AI 智能体**：可保存多套模型配置并指定一个默认，对话中随时切换；会话可绑定连接 / 库 / 表作为取数范围（工具调用缺省自动补齐），支持粘贴或拖入截图提问（OpenAI `image_url` / Responses `input_image` / Anthropic base64）；工具调用、对话历史、长期记忆、上下文压缩
- **外观**：深浅主题、一个全局字体与字号设置（界面与代码共用）、过渡动效并尊重 `prefers-reduced-motion`

## 数据目录

连接、会话、模型配置、记忆与日志保存在 `~/.agentdb/`：`connections.json`、`chat_sessions.json`、`ai_settings.json`（数组，含 `is_default`）、`memory.json`、`ai_images/`、`logs/`。
这是唯一的存储位置：程序不内置任何连接信息，删掉 `connections.json` 就等于从零开始新建连接。

项目由 db_tools 改名为 AgentDB，首次启动会把旧目录 `~/.db_tools/` 里的已知条目（上面这些文件与目录）一次性搬到 `~/.agentdb/`：只搬不覆盖，搬完旧目录空了才删，否则原样留着；界面上记在本机的偏好（主题、字体、模型选择等）同样从 `db_tools_*` 键自动迁到 `agentdb_*`。
