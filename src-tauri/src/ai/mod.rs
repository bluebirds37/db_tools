pub mod anthropic;
pub mod memory;
pub mod openai;
pub mod tools;

use crate::models::*;
use serde_json::{json, Value};

pub struct AgentContext {
    pub settings: AiSettings,
    pub memory_facts: Vec<String>,
    pub compressed_summary: Option<String>,
    /// 会话已选范围的可读描述，由命令层拼好
    pub scope_note: Option<String>,
}

pub fn build_system_prompt(ctx: &AgentContext) -> String {
    let mut sp = ctx.settings.system_prompt.clone();
    if let Some(scope) = &ctx.scope_note {
        sp.push_str("\n\n## 已选范围\n");
        sp.push_str(scope);
        sp.push_str(
            "\n调用工具时 connection_id / database 可以省略，会按这个范围补齐；\
            只有用户明确要换库或跨库时，才先调 list_connections / list_databases 确认。",
        );
    }
    if !ctx.memory_facts.is_empty() {
        sp.push_str("\n\n## 长期记忆\n");
        for f in &ctx.memory_facts {
            sp.push_str(&format!("- {f}\n"));
        }
    }
    if let Some(sum) = &ctx.compressed_summary {
        sp.push_str("\n\n## 早期对话摘要\n");
        sp.push_str(sum);
        sp.push('\n');
    }
    sp.push_str("\n\n## 当前可用工具\n");
    sp.push_str("- list_connections: 列出所有数据库连接\n");
    sp.push_str("- list_databases(connection_id): 列出库\n");
    sp.push_str("- list_tables(connection_id, database): 列出表/集合/索引\n");
    sp.push_str("- describe_table(connection_id, database, table): 表结构\n");
    sp.push_str("- query_data(connection_id, database, sql, page, page_size): 分页查询\n");
    sp.push_str("- execute_sql(connection_id, database, sql): 执行 SQL/命令\n");
    sp.push_str("- insert_row / update_row / delete_row: 增删改\n");
    sp
}

pub fn tool_definitions() -> Value {
    json!([
        {
            "name": "list_connections",
            "description": "列出所有已保存的数据库连接",
            "parameters": { "type": "object", "properties": {}, "required": [] }
        },
        {
            "name": "list_databases",
            "description": "列出指定连接下的数据库；connection_id 省略时用会话已选连接",
            "parameters": {
                "type": "object",
                "properties": {
                    "connection_id": { "type": "string", "description": "连接 ID，可省略" }
                },
                "required": []
            }
        },
        {
            "name": "list_tables",
            "description": "列出数据库中的表/集合/索引/键类型；connection_id、database 省略时用会话已选范围",
            "parameters": {
                "type": "object",
                "properties": {
                    "connection_id": { "type": "string" },
                    "database": { "type": "string" }
                },
                "required": []
            }
        },
        {
            "name": "describe_table",
            "description": "查看表结构（字段、主键、索引、外键）；连接与库可省略，按会话已选范围补齐",
            "parameters": {
                "type": "object",
                "properties": {
                    "connection_id": { "type": "string" },
                    "database": { "type": "string" },
                    "table": { "type": "string" }
                },
                "required": ["table"]
            }
        },
        {
            "name": "query_data",
            "description": "分页查询数据。SQL 数据库传 SQL；Redis 传 pattern；Mongo 传 collection|filterJson；ES 传 index|queryJson；Qdrant 传 collection|filterJson。connection_id、database 省略时用会话已选范围",
            "parameters": {
                "type": "object",
                "properties": {
                    "connection_id": { "type": "string" },
                    "database": { "type": "string", "description": "可为空" },
                    "sql": { "type": "string" },
                    "page": { "type": "integer", "description": "页码从 1 开始" },
                    "page_size": { "type": "integer" }
                },
                "required": ["sql"]
            }
        },
        {
            "name": "execute_sql",
            "description": "执行写操作或管理命令（DDL/DML/Redis 命令等）；连接与库可省略，按会话已选范围补齐",
            "parameters": {
                "type": "object",
                "properties": {
                    "connection_id": { "type": "string" },
                    "database": { "type": "string" },
                    "sql": { "type": "string" }
                },
                "required": ["sql"]
            }
        },
        {
            "name": "insert_row",
            "description": "插入一行/文档/key/向量点；连接与库可省略，按会话已选范围补齐",
            "parameters": {
                "type": "object",
                "properties": {
                    "connection_id": { "type": "string" },
                    "database": { "type": "string" },
                    "table": { "type": "string" },
                    "data": { "type": "object" }
                },
                "required": ["table", "data"]
            }
        },
        {
            "name": "update_row",
            "description": "按主键更新一行；连接与库可省略，按会话已选范围补齐",
            "parameters": {
                "type": "object",
                "properties": {
                    "connection_id": { "type": "string" },
                    "database": { "type": "string" },
                    "table": { "type": "string" },
                    "pk": { "type": "object" },
                    "data": { "type": "object" }
                },
                "required": ["table", "pk", "data"]
            }
        },
        {
            "name": "delete_row",
            "description": "按主键删除一行；连接与库可省略，按会话已选范围补齐",
            "parameters": {
                "type": "object",
                "properties": {
                    "connection_id": { "type": "string" },
                    "database": { "type": "string" },
                    "table": { "type": "string" },
                    "pk": { "type": "object" }
                },
                "required": ["table", "pk"]
            }
        },
        {
            "name": "remember_fact",
            "description": "将用户偏好或重要事实写入长期记忆",
            "parameters": {
                "type": "object",
                "properties": {
                    "content": { "type": "string" }
                },
                "required": ["content"]
            }
        }
    ])
}

pub fn openai_tools_payload() -> Value {
    let defs = tool_definitions();
    let arr = defs.as_array().cloned().unwrap_or_default();
    Value::Array(
        arr.into_iter()
            .map(|t| {
                json!({
                    "type": "function",
                    "function": {
                        "name": t["name"],
                        "description": t["description"],
                        "parameters": t["parameters"]
                    }
                })
            })
            .collect(),
    )
}

pub fn anthropic_tools_payload() -> Value {
    let defs = tool_definitions();
    let arr = defs.as_array().cloned().unwrap_or_default();
    Value::Array(
        arr.into_iter()
            .map(|t| {
                json!({
                    "name": t["name"],
                    "description": t["description"],
                    "input_schema": t["parameters"]
                })
            })
            .collect(),
    )
}

pub async fn chat_completion(
    settings: &AiSettings,
    messages: &[AiMessage],
    tools_json: Option<Value>,
) -> Result<AiChatResponse, String> {
    chat_completion_opts(settings, messages, tools_json, false).await
}

/// `force_tools`: use tool_choice=required / any so the model must emit tool calls.
pub async fn chat_completion_opts(
    settings: &AiSettings,
    messages: &[AiMessage],
    tools_json: Option<Value>,
    force_tools: bool,
) -> Result<AiChatResponse, String> {
    match settings.provider.as_str() {
        "anthropic" => {
            anthropic::complete(settings, messages, tools_json, force_tools).await
        }
        _ => openai::complete(settings, messages, tools_json, force_tools).await,
    }
}

/// Fallback: parse `tool_name(key=value, ...)` from assistant text when
/// the model describes tool calls instead of emitting tool_calls JSON.
pub fn parse_tool_calls_from_text(text: &str) -> Vec<crate::models::ToolCallRequest> {
    use crate::models::ToolCallRequest;
    use serde_json::{json, Value};
    const NAMES: [&str; 10] = [
        "list_connections",
        "list_databases",
        "list_tables",
        "describe_table",
        "query_data",
        "execute_sql",
        "insert_row",
        "update_row",
        "delete_row",
        "remember_fact",
    ];
    let mut out = Vec::new();
    for name in NAMES {
        let mut search_from = 0usize;
        while let Some(pos) = text[search_from..].find(name) {
            let abs = search_from + pos;
            let after = &text[abs + name.len()..];
            let rest = after.trim_start();
            if !rest.starts_with('(') {
                search_from = abs + name.len();
                continue;
            }
            // find matching close paren
            let body_start = abs + name.len() + (after.len() - rest.len()) + 1;
            let mut depth = 1usize;
            let mut end = None;
            for (i, ch) in text[body_start..].char_indices() {
                match ch {
                    '(' => depth += 1,
                    ')' => {
                        depth -= 1;
                        if depth == 0 {
                            end = Some(body_start + i);
                            break;
                        }
                    }
                    _ => {}
                }
            }
            let Some(end) = end else {
                break;
            };
            let args_str = &text[body_start..end];
            let mut map = serde_json::Map::new();
            for part in args_str.split(',') {
                let part = part.trim();
                if part.is_empty() {
                    continue;
                }
                if let Some((k, v)) = part.split_once('=') {
                    let k = k.trim().trim_matches('"').to_string();
                    let v = v.trim().trim_matches('"').to_string();
                    map.insert(k, Value::String(v));
                }
            }
            out.push(ToolCallRequest {
                id: format!("parsed-{}-{}", name, out.len()),
                name: name.to_string(),
                arguments: Value::Object(map),
            });
            search_from = end + 1;
            if out.len() >= 6 {
                return out;
            }
        }
    }
    let _ = json!({});
    out
}

/// Estimate rough tokens (chars/2 for mixed zh/en) and compress oldest messages.
pub async fn maybe_compress_history(
    settings: &AiSettings,
    messages: &[AiMessage],
    max_chars: usize,
) -> Result<(Vec<AiMessage>, Option<String>), String> {
    let total: usize = messages.iter().map(|m| m.content.len()).sum();
    if total < max_chars {
        return Ok((messages.to_vec(), None));
    }
    // keep last 8 messages, summarize the rest
    let keep = 8.min(messages.len().saturating_sub(2));
    let split_at = messages.len().saturating_sub(keep);
    if split_at == 0 {
        return Ok((messages.to_vec(), None));
    }
    let older = &messages[..split_at];
    let recent = messages[split_at..].to_vec();

    let mut summary_prompt = vec![AiMessage::text(
        "system",
        "请将以下对话压缩为简洁的中文摘要，保留关键数据库操作、表名、SQL 结论和用户偏好。不超过 400 字。",
    )];
    for m in older {
        // 摘要是纯文本任务，图片不带过去
        summary_prompt.push(AiMessage::text(&m.role, m.content.chars().take(500).collect::<String>()));
    }
    let summary = chat_completion(settings, &summary_prompt, None).await?;
    Ok((recent, Some(summary.content)))
}
