use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum DbKind {
    Mysql,
    Postgres,
    Sqlite,
    Redis,
    Mongodb,
    Elasticsearch,
    Qdrant,
    Pgvector,
}

impl DbKind {
    #[allow(dead_code)]
    pub fn label(&self) -> &'static str {
        match self {
            DbKind::Mysql => "MySQL",
            DbKind::Postgres => "PostgreSQL",
            DbKind::Sqlite => "SQLite",
            DbKind::Redis => "Redis",
            DbKind::Mongodb => "MongoDB",
            DbKind::Elasticsearch => "Elasticsearch",
            DbKind::Qdrant => "Qdrant",
            DbKind::Pgvector => "PgVector",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionConfig {
    pub id: String,
    pub name: String,
    pub kind: DbKind,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub username: Option<String>,
    pub password: Option<String>,
    pub database: Option<String>,
    /// Extra params (redis db index, mongo authSource, sqlite file path, etc.)
    #[serde(default)]
    pub options: HashMap<String, String>,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableInfo {
    pub name: String,
    pub schema: Option<String>,
    pub table_type: Option<String>,
    pub comment: Option<String>,
    pub row_estimate: Option<i64>,
    #[serde(default)]
    pub tablespace: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TablespaceInfo {
    pub name: String,
    pub owner: Option<String>,
    pub location: Option<String>,
    pub size: Option<String>,
    pub comment: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableExtra {
    pub schema: Option<String>,
    pub tablespace: Option<String>,
    pub size: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColumnInfo {
    pub name: String,
    pub data_type: String,
    pub nullable: bool,
    pub primary_key: bool,
    pub default: Option<String>,
    pub comment: Option<String>,
    pub extra: Option<String>,
    pub max_length: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexInfo {
    pub name: String,
    pub columns: Vec<String>,
    pub unique: bool,
    pub primary: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForeignKeyInfo {
    pub name: String,
    pub column: String,
    pub ref_table: String,
    pub ref_column: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryPage {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<serde_json::Value>>,
    pub total: u64,
    pub page: u64,
    pub page_size: u64,
    pub elapsed_ms: u64,
    /// For non-SQL sources: display message
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecResult {
    pub affected_rows: u64,
    pub last_insert_id: Option<i64>,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct DbStructure {
    pub database: String,
    pub tables: Vec<TableInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiSettings {
    /// 多配置标识，空表示旧版单配置
    #[serde(default)]
    pub id: String,
    /// 用户起的名字，智能体切换列表里显示
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub is_default: bool,
    pub provider: String,
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub temperature: f32,
    pub max_tokens: u32,
    pub system_prompt: String,
    pub enable_tools: bool,
    /// chat_completions | responses（OpenAI Responses API）
    #[serde(default = "default_api_style")]
    pub api_style: String,
    /// auto | text | json_object
    #[serde(default = "default_response_mode")]
    pub response_mode: String,
}

impl AiSettings {
    /// 列表里没有名字时退化成「模型名（provider）」
    pub fn display_name(&self) -> String {
        if !self.name.trim().is_empty() {
            return self.name.trim().to_string();
        }
        let m = if self.model.trim().is_empty() {
            "未填模型"
        } else {
            self.model.trim()
        };
        format!("{m}（{provider}）", provider = if self.provider == "anthropic" { "Anthropic" } else { "OpenAI 兼容" })
    }
}

fn default_api_style() -> String {
    "chat_completions".into()
}
fn default_response_mode() -> String {
    "auto".into()
}

impl Default for AiSettings {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            is_default: true,
            provider: "openai".into(),
            base_url: "https://api.openai.com/v1".into(),
            api_key: String::new(),
            model: "gpt-4o-mini".into(),
            temperature: 0.3,
            max_tokens: 4096,
            system_prompt: default_system_prompt(),
            enable_tools: true,
            api_style: default_api_style(),
            response_mode: default_response_mode(),
        }
    }
}

pub fn default_system_prompt() -> String {
    "你是 AgentDB 的内置数据库智能助手。你可以帮助用户管理连接、浏览库表、执行 SQL 与增删改查。\n\
     回答请使用中文，简洁准确。执行写操作前请确认表结构。优先使用提供的工具完成任务。"
        .into()
}

// ---------- 结构页 AI 补全 ----------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TableEnrichment {
    /// 建议的表 / 集合注释，空表示不改
    #[serde(default)]
    pub table_comment: String,
    #[serde(default)]
    pub column_comments: Vec<ColumnComment>,
    /// 现有索引的一句话说明
    #[serde(default)]
    pub index_notes: Vec<IndexNote>,
    #[serde(default)]
    pub suggested_indexes: Vec<SuggestedIndex>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColumnComment {
    pub name: String,
    pub comment: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexNote {
    pub name: String,
    pub note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuggestedIndex {
    pub name: String,
    pub columns: Vec<String>,
    #[serde(default)]
    pub unique: bool,
    #[serde(default)]
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub id: String,
    pub role: String,
    pub content: String,
    pub created_at: String,
    #[serde(default)]
    pub tool_calls: Option<serde_json::Value>,
    /// 图片文件名（存在 data_dir/ai_images 下），会话里不落 base64
    #[serde(default)]
    pub images: Vec<String>,
}

/// 会话绑定的取数范围：智能体不用再自行翻连接列表
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ChatScope {
    pub connection_id: Option<String>,
    pub database: Option<String>,
    pub table: Option<String>,
}

impl ChatScope {
    pub fn is_empty(&self) -> bool {
        self.connection_id.is_none() && self.database.is_none() && self.table.is_none()
    }
}

/// 发给模型的一条消息；`images` 是 data URL，由 provider 层拼成多模态 content
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiMessage {
    pub role: String,
    pub content: String,
    #[serde(default)]
    pub images: Vec<String>,
}

impl AiMessage {
    pub fn text(role: &str, content: impl Into<String>) -> Self {
        Self {
            role: role.into(),
            content: content.into(),
            images: vec![],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatSession {
    pub id: String,
    pub title: String,
    pub messages: Vec<ChatMessage>,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default)]
    pub connection_id: Option<String>,
    #[serde(default)]
    pub database: Option<String>,
    #[serde(default)]
    pub table: Option<String>,
}

impl ChatSession {
    pub fn scope(&self) -> ChatScope {
        ChatScope {
            connection_id: self.connection_id.clone(),
            database: self.database.clone(),
            table: self.table.clone(),
        }
    }

    /// 删除会话时要一并丢掉的图片文件名
    pub fn image_files(&self) -> Vec<String> {
        self.messages.iter().flat_map(|m| m.images.clone()).collect()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MemoryStore {
    /// long-term facts
    pub facts: Vec<MemoryFact>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryFact {
    pub id: String,
    pub content: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallRequest {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiChatResponse {
    pub content: String,
    pub tool_calls: Vec<ToolCallRequest>,
    pub finish_reason: String,
}

// ---------- 宽松反序列化：前端会把数字/布尔序列化成字符串 ----------

fn de_opt_str<'de, D>(d: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::Deserialize;
    let v = Option::<serde_json::Value>::deserialize(d)?;
    Ok(match v {
        None | Some(serde_json::Value::Null) => None,
        Some(serde_json::Value::String(s)) => {
            let t = s.trim();
            if t.is_empty() {
                None
            } else {
                Some(t.to_string())
            }
        }
        Some(other) => Some(other.to_string().trim_matches('"').to_string()),
    })
}

fn de_opt_i64<'de, D>(d: D) -> Result<Option<i64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::Deserialize;
    let v = Option::<serde_json::Value>::deserialize(d)?;
    Ok(match v {
        None | Some(serde_json::Value::Null) => None,
        Some(serde_json::Value::Number(n)) => n.as_i64().or_else(|| n.to_string().parse().ok()),
        Some(serde_json::Value::Bool(b)) => Some(if b { 1 } else { 0 }),
        Some(serde_json::Value::String(s)) => {
            let t = s.trim();
            if t.is_empty() {
                None
            } else {
                t.parse().ok()
            }
        }
        _ => None,
    })
}

fn de_opt_bool<'de, D>(d: D) -> Result<Option<bool>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::Deserialize;
    let v = Option::<serde_json::Value>::deserialize(d)?;
    Ok(match v {
        None | Some(serde_json::Value::Null) => None,
        Some(serde_json::Value::Bool(b)) => Some(b),
        Some(serde_json::Value::Number(n)) => Some(n.as_f64().map(|f| f != 0.0).unwrap_or(false)),
        Some(serde_json::Value::String(s)) => match s.trim().to_ascii_lowercase().as_str() {
            "" => None,
            "true" | "1" | "yes" | "on" => Some(true),
            "false" | "0" | "no" | "off" => Some(false),
            _ => None,
        },
        _ => None,
    })
}

/// 新建 / 编辑数据库的表单值
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct DbSpec {
    pub name: String,
    #[serde(deserialize_with = "de_opt_str")]
    pub new_name: Option<String>,
    #[serde(deserialize_with = "de_opt_str")]
    pub owner: Option<String>,
    #[serde(deserialize_with = "de_opt_str")]
    pub template: Option<String>,
    #[serde(deserialize_with = "de_opt_str")]
    pub encoding: Option<String>,
    #[serde(deserialize_with = "de_opt_str")]
    pub collate: Option<String>,
    #[serde(deserialize_with = "de_opt_str")]
    pub ctype: Option<String>,
    #[serde(deserialize_with = "de_opt_str")]
    pub tablespace: Option<String>,
    #[serde(deserialize_with = "de_opt_i64")]
    pub conn_limit: Option<i64>,
    #[serde(deserialize_with = "de_opt_bool")]
    pub allow_connect: Option<bool>,
    #[serde(deserialize_with = "de_opt_bool")]
    pub is_template: Option<bool>,
    #[serde(deserialize_with = "de_opt_str")]
    pub comment: Option<String>,
    #[serde(deserialize_with = "de_opt_str")]
    pub charset: Option<String>,
    #[serde(deserialize_with = "de_opt_str")]
    pub collation: Option<String>,
}

/// 该引擎的数据库设计器支持哪些字段
#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DbCaps {
    pub owner: bool,
    pub template: bool,
    pub encoding: bool,
    pub collate: bool,
    pub ctype: bool,
    pub tablespace: bool,
    pub conn_limit: bool,
    pub allow_connect: bool,
    pub is_template: bool,
    pub comment: bool,
    pub charset: bool,
    pub extensions: bool,
    /// 建库后不可改的字段，编辑态要禁用
    pub immutable_on_alter: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DbOptions {
    pub caps: DbCaps,
    pub owners: Vec<String>,
    pub templates: Vec<String>,
    pub encodings: Vec<String>,
    pub collations: Vec<String>,
    pub tablespaces: Vec<String>,
    pub charsets: Vec<String>,
    pub collation_names: Vec<String>,
    pub current: Option<DbSpec>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtInfo {
    pub name: String,
    pub default_version: String,
    pub installed_version: Option<String>,
    pub comment: Option<String>,
    pub schema: Option<String>,
}

/// 建表用的一列定义
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct ColSpec {
    #[serde(default)]
    pub name: String,
    #[serde(deserialize_with = "de_opt_str")]
    pub data_type: Option<String>,
    #[serde(deserialize_with = "de_opt_bool")]
    pub nullable: Option<bool>,
    #[serde(deserialize_with = "de_opt_bool")]
    pub primary_key: Option<bool>,
    #[serde(deserialize_with = "de_opt_bool")]
    pub auto_increment: Option<bool>,
    #[serde(deserialize_with = "de_opt_str")]
    pub default_value: Option<String>,
    #[serde(deserialize_with = "de_opt_str")]
    pub comment: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct TableSpec {
    pub name: String,
    #[serde(default)]
    pub columns: Vec<ColSpec>,
    #[serde(deserialize_with = "de_opt_str")]
    pub comment: Option<String>,
    #[serde(deserialize_with = "de_opt_str")]
    pub engine: Option<String>,
    #[serde(deserialize_with = "de_opt_str")]
    pub charset: Option<String>,
}

/// 迁移选项：结构与数据默认都开，目标已存在默认跳过
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MigrateOptions {
    pub with_structure: bool,
    pub with_data: bool,
    pub with_indexes: bool,
    pub on_exists: String,
    pub max_rows_per_table: i64,
}

impl Default for MigrateOptions {
    fn default() -> Self {
        Self {
            with_structure: true,
            with_data: true,
            with_indexes: true,
            on_exists: "skip".into(),
            max_rows_per_table: 200_000,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrateTableResult {
    pub table: String,
    /// created | copied | skipped | failed
    pub status: String,
    pub rows: i64,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct MigrateReport {
    pub tables: Vec<MigrateTableResult>,
    pub total_rows: i64,
    pub created: i64,
    pub copied: i64,
    pub skipped: i64,
    pub failed: i64,
}

/// 单表同步结果，字段与 MigrateTableResult 对齐，前端共用一套展示
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveProfileResult {
    /// 带正式 id 的已保存配置
    pub profile: AiSettings,
    pub profiles: Vec<AiSettings>,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SyncTableResult {
    pub table: String,
    /// created | copied | failed
    pub status: String,
    pub rows: i64,
    pub failed_rows: i64,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SyncReport {
    pub tables: Vec<SyncTableResult>,
    pub total_rows: i64,
    pub failed_rows: i64,
    pub created: i64,
    pub copied: i64,
    pub failed: i64,
    /// 一句话结论，前端 notice 条直接用
    pub message: String,
}

/// 导出脱敏规则：列名大小写不敏感，可选 `表.列` / 反引号写法
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct MaskRule {
    pub column: String,
    /// none | mask-all | keep-edges | email | phone | idcard | bankcard | name | hash | null | replace
    pub mode: String,
    #[serde(deserialize_with = "de_opt_i64")]
    pub keep_head: Option<i64>,
    #[serde(deserialize_with = "de_opt_i64")]
    pub keep_tail: Option<i64>,
    #[serde(deserialize_with = "de_opt_str")]
    pub mask_text: Option<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ExportResult {
    pub content: String,
    pub rows: i64,
    /// 实际命中并被改写的列
    pub masked_columns: Vec<String>,
    /// 规则里写了但结果集中不存在的列
    pub unmatched_columns: Vec<String>,
}
