# 01 · 快速开始

适用版本：0.1.0 ｜ 更新于 2026-10-10

## 环境要求

- Node.js 18 / 20 / 22+（vite 6 的要求）
- 稳定的 Rust 工具链；Windows 需要 MSVC 链接器与 WebView2 运行时
- 想连本机测试库时，需要相应的数据库服务在跑（端口见 [02 · 连接管理](02-connections.md)）

## 跑起来

```bash
npm install
npm run tauri dev      # 前端 vite 固定端口 44100，改代码即时热更新
```

只构建前端：

```bash
npm run build          # 类型检查（tsc，noEmit）+ vite build -> dist/
```

后端不启窗口的手工验证（需要本机容器）：

```bash
cd src-tauri
cargo check --examples
cargo run --example full_test
```

打包与三平台产物见根目录 [`README.md`](../README.md) 的「打包与分发」。

## 界面分区

![总览](../images/01-overview.png)

| 区域 | 作用 |
| --- | --- |
| 左侧「连接」 | 连接列表与展开后的库 / 表树，顶部可搜索连接名，右上 `+` 新建连接 |
| 左下状态条 | 本机服务状态与「日志」入口 |
| 顶栏 | 当前路径（连接 › 库 › 表）、工具区、AI 面板开关、主题切换、设置 |
| 顶栏工具区 | 数据库同步 / 数据库迁移 / 数据导出 / 数据库备份，未满足前置条件时置灰并在悬停提示里说明缺什么 |
| 中部 | 页签：数据 / 结构 / SQL（非关系引擎按各自形态改名，见 [05](05-non-sql-engines.md)） |
| 右侧 | AI 智能体面板，可收起 |

## 常用快捷键

| 按键 | 作用 |
| --- | --- |
| `Ctrl` / `Cmd` + `Enter` | 在 SQL / 命令 / REST 输入框里执行 |
| 下拉中 `↑` `↓` `Enter` `Esc` | 在可筛选下拉里移动、确认、关闭 |
| 下拉中直接打字 | 按子串或子序列过滤候选（例如输入 `uid` 命中 `user_id`） |

## 本机数据与日志

配置都存在 `~/.db_tools/`：`connections.json`、`chat_sessions.json`、`ai_settings.json`、`memory.json`，日志在同目录 `logs/`，也可点左下「日志」查看尾部。应用运行时的建连、执行、失败原因都会写进日志，界面上的报错与日志内容一致。
