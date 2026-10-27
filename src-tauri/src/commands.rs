use crate::ai::tools::{execute_tool, get_session, SharedCache};
use crate::ai::{build_system_prompt, chat_completion, maybe_compress_history, AgentContext};
use crate::db;
use crate::logger;
use crate::models::*;
use crate::store::{lock_or_recover, AppState};
use chrono::Utc;
use serde_json::{json, Value};
use tauri::State;
use uuid::Uuid;

fn now() -> String {
    Utc::now().to_rfc3339()
}

fn log_err(cmd: &str, connection_id: &str, e: &str) -> String {
    logger::error(cmd, format!("conn={connection_id} err={e}"));
    e.to_string()
}

fn log_ok(cmd: &str, connection_id: &str, detail: &str) {
    logger::info(cmd, format!("conn={connection_id} {detail}"));
}

// ---------- Connections ----------

#[tauri::command]
pub fn list_connections(state: State<AppState>) -> Vec<ConnectionConfig> {
    lock_or_recover(&state.connections).clone()
}

#[tauri::command]
pub fn save_connection(
    state: State<AppState>,
    mut config: ConnectionConfig,
) -> Result<ConnectionConfig, String> {
    if config.id.is_empty() {
        config.id = Uuid::new_v4().to_string();
        config.created_at = Some(now());
    }
    config.updated_at = Some(now());
    {
        let mut list = lock_or_recover(&state.connections);
        if let Some(pos) = list.iter().position(|c| c.id == config.id) {
            list[pos] = config.clone();
        } else {
            list.push(config.clone());
        }
    }
    state.save_connections();
    logger::info(
        "save_connection",
        format!("id={} name={} kind={:?}", config.id, config.name, config.kind),
    );
    Ok(config)
}

#[tauri::command]
pub fn delete_connection(state: State<AppState>, id: String) -> Result<(), String> {
    {
        let mut list = lock_or_recover(&state.connections);
        list.retain(|c| c.id != id);
    }
    state.save_connections();
    logger::info("delete_connection", format!("id={id}"));
    Ok(())
}

#[tauri::command]
pub async fn test_connection(config: ConnectionConfig) -> Result<bool, String> {
    let session = db::open_session(&config)
        .await
        .map_err(|e| log_err("test_connection", &config.id, &e))?;
    session
        .ping()
        .await
        .map_err(|e| log_err("test_connection.ping", &config.id, &e))?;
    log_ok("test_connection", &config.id, "ok");
    Ok(true)
}

// ---------- Schema / Data ----------

#[tauri::command]
pub async fn list_databases(
    state: State<'_, AppState>,
    cache: State<'_, SharedCache>,
    connection_id: String,
) -> Result<Vec<String>, String> {
    let s = get_session(&cache, &state, &connection_id)
        .await
        .map_err(|e| log_err("list_databases.session", &connection_id, &e))?;
    s.list_databases()
        .await
        .map_err(|e| log_err("list_databases", &connection_id, &e))
}

#[tauri::command]
pub async fn list_tables(
    state: State<'_, AppState>,
    cache: State<'_, SharedCache>,
    connection_id: String,
    database: String,
    schema: Option<String>,
) -> Result<Vec<TableInfo>, String> {
    let s = get_session(&cache, &state, &connection_id)
        .await
        .map_err(|e| log_err("list_tables.session", &connection_id, &e))?;
    s.list_tables(&database, schema.as_deref())
        .await
        .map_err(|e| log_err("list_tables", &connection_id, &format!("db={database} {e}")))
}

#[tauri::command]
pub async fn list_columns(
    state: State<'_, AppState>,
    cache: State<'_, SharedCache>,
    connection_id: String,
    database: String,
    table: String,
    schema: Option<String>,
) -> Result<Vec<ColumnInfo>, String> {
    let s = get_session(&cache, &state, &connection_id)
        .await
        .map_err(|e| log_err("list_columns.session", &connection_id, &e))?;
    s.list_columns(&database, schema.as_deref(), &table)
        .await
        .map_err(|e| {
        log_err(
            "list_columns",
            &connection_id,
            &format!("db={database} table={table} {e}"),
        )
    })
}

#[tauri::command]
pub async fn list_indexes(
    state: State<'_, AppState>,
    cache: State<'_, SharedCache>,
    connection_id: String,
    database: String,
    table: String,
    schema: Option<String>,
) -> Result<Vec<IndexInfo>, String> {
    let s = get_session(&cache, &state, &connection_id)
        .await
        .map_err(|e| log_err("list_indexes.session", &connection_id, &e))?;
    s.list_indexes(&database, schema.as_deref(), &table)
        .await
        .map_err(|e| {
        log_err(
            "list_indexes",
            &connection_id,
            &format!("db={database} table={table} {e}"),
        )
    })
}

#[tauri::command]
pub async fn list_foreign_keys(
    state: State<'_, AppState>,
    cache: State<'_, SharedCache>,
    connection_id: String,
    database: String,
    table: String,
    schema: Option<String>,
) -> Result<Vec<ForeignKeyInfo>, String> {
    let s = get_session(&cache, &state, &connection_id)
        .await
        .map_err(|e| log_err("list_foreign_keys.session", &connection_id, &e))?;
    s.list_foreign_keys(&database, schema.as_deref(), &table)
        .await
        .map_err(|e| {
        log_err(
            "list_foreign_keys",
            &connection_id,
            &format!("db={database} table={table} {e}"),
        )
    })
}

#[tauri::command]
pub async fn list_schemas(
    state: State<'_, AppState>,
    cache: State<'_, SharedCache>,
    connection_id: String,
    database: String,
) -> Result<Vec<String>, String> {
    let s = get_session(&cache, &state, &connection_id)
        .await
        .map_err(|e| log_err("list_schemas.session", &connection_id, &e))?;
    s.list_schemas(&database)
        .await
        .map_err(|e| log_err("list_schemas", &connection_id, &e))
}

#[tauri::command]
pub async fn list_tablespaces(
    state: State<'_, AppState>,
    cache: State<'_, SharedCache>,
    connection_id: String,
) -> Result<Vec<TablespaceInfo>, String> {
    let s = get_session(&cache, &state, &connection_id)
        .await
        .map_err(|e| log_err("list_tablespaces.session", &connection_id, &e))?;
    s.list_tablespaces()
        .await
        .map_err(|e| log_err("list_tablespaces", &connection_id, &e))
}

#[tauri::command]
pub async fn describe_table_extra(
    state: State<'_, AppState>,
    cache: State<'_, SharedCache>,
    connection_id: String,
    database: String,
    table: String,
    schema: Option<String>,
) -> Result<TableExtra, String> {
    let s = get_session(&cache, &state, &connection_id)
        .await
        .map_err(|e| log_err("describe_table_extra.session", &connection_id, &e))?;
    s.describe_table_extra(&database, schema.as_deref(), &table)
        .await
        .map_err(|e| {
            log_err(
                "describe_table_extra",
                &connection_id,
                &format!("db={database} table={table} {e}"),
            )
        })
}

#[tauri::command]
pub async fn query_page(
    state: State<'_, AppState>,
    cache: State<'_, SharedCache>,
    connection_id: String,
    database: String,
    sql: String,
    page: u64,
    page_size: u64,
) -> Result<QueryPage, String> {
    let s = get_session(&cache, &state, &connection_id)
        .await
        .map_err(|e| log_err("query_page.session", &connection_id, &e))?;
    let res = s
        .query_page(&database, &sql, page.max(1), page_size.clamp(1, 500))
        .await
        .map_err(|e| {
            let preview: String = sql.chars().take(120).collect();
            log_err(
                "query_page",
                &connection_id,
                &format!("db={database} sql={preview} {e}"),
            )
        })?;
    log_ok(
        "query_page",
        &connection_id,
        &format!(
            "db={database} rows={} total={} ms={}",
            res.rows.len(),
            res.total,
            res.elapsed_ms
        ),
    );
    Ok(res)
}

#[tauri::command]
pub async fn execute_sql(
    state: State<'_, AppState>,
    cache: State<'_, SharedCache>,
    connection_id: String,
    database: String,
    sql: String,
) -> Result<ExecResult, String> {
    let s = get_session(&cache, &state, &connection_id)
        .await
        .map_err(|e| log_err("execute_sql.session", &connection_id, &e))?;
    let res = s.execute(&database, &sql).await.map_err(|e| {
        let preview: String = sql.chars().take(120).collect();
        log_err(
            "execute_sql",
            &connection_id,
            &format!("db={database} sql={preview} {e}"),
        )
    })?;
    log_ok(
        "execute_sql",
        &connection_id,
        &format!("db={database} affected={}", res.affected_rows),
    );
    Ok(res)
}

#[tauri::command]
pub async fn insert_row(
    state: State<'_, AppState>,
    cache: State<'_, SharedCache>,
    connection_id: String,
    database: String,
    table: String,
    data: Value,
) -> Result<ExecResult, String> {
    let s = get_session(&cache, &state, &connection_id)
        .await
        .map_err(|e| log_err("insert_row.session", &connection_id, &e))?;
    let map = data.as_object().cloned().unwrap_or_default();
    s.insert_row(&database, &table, map)
        .await
        .map_err(|e| {
            log_err(
                "insert_row",
                &connection_id,
                &format!("db={database} table={table} {e}"),
            )
        })
}

#[tauri::command]
pub async fn update_row(
    state: State<'_, AppState>,
    cache: State<'_, SharedCache>,
    connection_id: String,
    database: String,
    table: String,
    pk: Value,
    data: Value,
) -> Result<ExecResult, String> {
    let s = get_session(&cache, &state, &connection_id)
        .await
        .map_err(|e| log_err("update_row.session", &connection_id, &e))?;
    let pk_map = pk.as_object().cloned().unwrap_or_default();
    let data_map = data.as_object().cloned().unwrap_or_default();
    s.update_row(&database, &table, pk_map, data_map)
        .await
        .map_err(|e| {
            log_err(
                "update_row",
                &connection_id,
                &format!("db={database} table={table} {e}"),
            )
        })
}

#[tauri::command]
pub async fn delete_row(
    state: State<'_, AppState>,
    cache: State<'_, SharedCache>,
    connection_id: String,
    database: String,
    table: String,
    pk: Value,
) -> Result<ExecResult, String> {
    let s = get_session(&cache, &state, &connection_id)
        .await
        .map_err(|e| log_err("delete_row.session", &connection_id, &e))?;
    let pk_map = pk.as_object().cloned().unwrap_or_default();
    s.delete_row(&database, &table, pk_map).await.map_err(|e| {
        log_err(
            "delete_row",
            &connection_id,
            &format!("db={database} table={table} {e}"),
        )
    })
}

#[tauri::command]
pub async fn reconnect(
    cache: State<'_, SharedCache>,
    connection_id: String,
) -> Result<(), String> {
    cache.invalidate(&connection_id);
    Ok(())
}

// ---------- AI Settings（多配置，一个默认） ----------

#[tauri::command]
pub fn list_ai_profiles(state: State<AppState>) -> Vec<AiSettings> {
    lock_or_recover(&state.ai_profiles).clone()
}

/// 写入后必须仍能确定唯一默认，否则智能体不知道该用哪条
fn store_profiles(state: &State<AppState>, list: Vec<AiSettings>) -> Vec<AiSettings> {
    let fixed = crate::store::normalize_profiles(list);
    *lock_or_recover(&state.ai_profiles) = fixed.clone();
    state.save_ai_profiles();
    fixed
}

#[tauri::command]
pub fn save_ai_profile(
    state: State<AppState>,
    profile: AiSettings,
) -> Result<SaveProfileResult, String> {
    if profile.base_url.trim().is_empty() {
        return Err("请填写 Base URL".into());
    }
    if profile.model.trim().is_empty() {
        return Err("请填写模型名".into());
    }
    let mut saved = profile.clone();
    if saved.id.is_empty() {
        saved.id = Uuid::new_v4().to_string();
    }
    let list = lock_or_recover(&state.ai_profiles).clone();
    let list = store_profiles(&state, crate::store::upsert_profile(&list, saved.clone()));
    logger::info(
        "save_ai_profile",
        format!(
            "profile={} provider={} model={}",
            saved.display_name(),
            saved.provider,
            saved.model
        ),
    );
    let saved = list
        .iter()
        .find(|p| p.id == saved.id)
        .cloned()
        .unwrap_or(saved);
    Ok(SaveProfileResult {
        profile: saved,
        profiles: list,
    })
}

#[tauri::command]
pub fn delete_ai_profile(
    state: State<AppState>,
    id: String,
) -> Result<Vec<AiSettings>, String> {
    let list = lock_or_recover(&state.ai_profiles).clone();
    if !list.iter().any(|p| p.id == id) {
        return Err(format!("配置不存在：{id}"));
    }
    Ok(store_profiles(
        &state,
        crate::store::remove_profile(&list, &id),
    ))
}

#[tauri::command]
pub fn set_default_ai_profile(
    state: State<AppState>,
    id: String,
) -> Result<Vec<AiSettings>, String> {
    let list = lock_or_recover(&state.ai_profiles).clone();
    if !list.iter().any(|p| p.id == id) {
        return Err(format!("配置不存在：{id}"));
    }
    Ok(store_profiles(
        &state,
        crate::store::mark_default(&list, &id),
    ))
}

/// 智能体取配置：显式指定 > 默认 > 第一条
pub(crate) fn resolve_ai_profile(
    state: &AppState,
    id: Option<&str>,
) -> Result<AiSettings, String> {
    let list = lock_or_recover(&state.ai_profiles);
    crate::store::resolve_profile(&list, id)
        .ok_or_else(|| "请先在「设置 → 大模型」中添加一个模型配置".to_string())
}

// ---------- Chat Sessions ----------

#[tauri::command]
pub fn list_chat_sessions(state: State<AppState>) -> Vec<ChatSession> {
    lock_or_recover(&state.sessions).clone()
}

#[tauri::command]
pub fn create_chat_session(state: State<AppState>, title: Option<String>) -> ChatSession {
    let s = ChatSession {
        id: Uuid::new_v4().to_string(),
        title: title.unwrap_or_else(|| "新对话".into()),
        messages: vec![],
        created_at: now(),
        updated_at: now(),
        connection_id: None,
        database: None,
        table: None,
    };
    lock_or_recover(&state.sessions).insert(0, s.clone());
    state.save_sessions();
    s
}

/// 会话绑定的取数范围：面板改一下就用，不发消息也要留住
#[tauri::command]
pub fn set_chat_scope(
    state: State<AppState>,
    id: String,
    connection_id: Option<String>,
    database: Option<String>,
    table: Option<String>,
) -> Result<ChatSession, String> {
    let clean = |v: &Option<String>| {
        v.as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    };
    let mut sessions = lock_or_recover(&state.sessions);
    let s = sessions.iter_mut().find(|s| s.id == id).ok_or("会话不存在")?;
    s.connection_id = clean(&connection_id);
    s.database = clean(&database);
    s.table = clean(&table);
    s.updated_at = now();
    let saved = s.clone();
    drop(sessions);
    state.save_sessions();
    Ok(saved)
}

/// 历史消息里的缩略图：文件名换成 data URL 再给前端
#[tauri::command]
pub fn ai_image_data_uri(state: State<AppState>, name: String) -> Result<String, String> {
    image_data_uri(&state, &name)
}

#[tauri::command]
pub fn delete_chat_session(state: State<AppState>, id: String) -> Result<(), String> {
    let images = {
        let sessions = lock_or_recover(&state.sessions);
        sessions
            .iter()
            .find(|s| s.id == id)
            .map(|s| s.image_files())
            .unwrap_or_default()
    };
    lock_or_recover(&state.sessions).retain(|s| s.id != id);
    state.save_sessions();
    state.remove_ai_images(&images);
    Ok(())
}

#[tauri::command]
pub fn rename_chat_session(
    state: State<AppState>,
    id: String,
    title: String,
) -> Result<(), String> {
    if let Some(s) = lock_or_recover(&state.sessions)
        .iter_mut()
        .find(|s| s.id == id)
    {
        s.title = title;
        s.updated_at = now();
    }
    state.save_sessions();
    Ok(())
}

// ---------- Memory ----------

#[tauri::command]
pub fn list_memory_facts(state: State<AppState>) -> Vec<MemoryFact> {
    lock_or_recover(&state.memory).facts.clone()
}

#[tauri::command]
pub fn add_memory_fact(state: State<AppState>, content: String) -> Result<(), String> {
    crate::ai::memory::add_fact(&mut lock_or_recover(&state.memory), &content);
    state.save_memory();
    Ok(())
}

#[tauri::command]
pub fn clear_memory_facts(state: State<AppState>) -> Result<(), String> {
    lock_or_recover(&state.memory).facts.clear();
    state.save_memory();
    Ok(())
}

fn connection_snapshot(state: &AppState) -> String {
    let list = lock_or_recover(&state.connections);
    if list.is_empty() {
        return "（当前没有已保存的数据库连接）".into();
    }
    let mut s = String::from("当前数据库连接：\n");
    for c in list.iter() {
        let loc = if c.kind == DbKind::Sqlite {
            c.options
                .get("path")
                .cloned()
                .unwrap_or_else(|| "本地文件".into())
        } else {
            format!(
                "{}:{}",
                c.host.clone().unwrap_or_else(|| "localhost".into()),
                c.port.unwrap_or(0)
            )
        };
        s.push_str(&format!(
            "- id={} | 名称={} | 类型={:?} | {}\n",
            c.id, c.name, c.kind, loc
        ));
    }
    s.push_str("请使用工具（或上述 id）完成用户的数据库操作请求。");
    s
}

fn looks_like_db_action(msg: &str) -> bool {
    const KEYS: [&str; 24] = [
        "查询", "查一下", "列出", "显示", "查看", "表", "库", "数据库", "SQL", "sql",
        "连接", "执行", "插入", "删除", "更新", "修改", "导出", "转储", "同步",
        "users", "SELECT", "select", "redis", "mongo",
    ];
    KEYS.iter().any(|k| msg.contains(k))
}

// ---------- AI 图片 ----------

/// 一条消息最多几张图
const MAX_AI_IMAGES: usize = 4;
/// 单张解码后上限，防止一次请求把内存与网关都撑爆
const MAX_AI_IMAGE_BYTES: usize = 8 * 1024 * 1024;
/// 追问时最多把最近几张历史图片重新附带（模型不会记住上一轮的图）
const MAX_HISTORY_IMAGES: usize = 4;

fn parse_data_url(u: &str) -> Option<(String, Vec<u8>)> {
    let rest = u.strip_prefix("data:")?;
    let (mime, b64) = rest.split_once(";base64,")?;
    if !mime.starts_with("image/") || b64.is_empty() {
        return None;
    }
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(b64.as_bytes())
        .ok()?;
    Some((mime.to_string(), bytes))
}

fn mime_ext(mime: &str) -> &'static str {
    match mime {
        "image/png" => "png",
        "image/jpeg" | "image/jpg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "image/bmp" => "bmp",
        _ => "bin",
    }
}

fn ext_mime(ext: &str) -> &'static str {
    match ext {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        _ => "application/octet-stream",
    }
}

/// 磁盘上的图片 → data URL；读不到就返回 Err，让调用方决定跳过还是报错
fn image_data_uri(state: &AppState, name: &str) -> Result<String, String> {
    let bytes = state.read_ai_image(name)?;
    use base64::Engine;
    let ext = name.rsplit('.').next().unwrap_or("");
    Ok(format!(
        "data:{};base64,{}",
        ext_mime(ext),
        base64::engine::general_purpose::STANDARD.encode(&bytes)
    ))
}

/// 会话已选范围写成提示词
fn scope_note(state: &AppState, scope: &ChatScope) -> Option<String> {
    if scope.is_empty() {
        return None;
    }
    let mut s = String::new();
    if let Some(cid) = &scope.connection_id {
        let conn = lock_or_recover(&state.connections).iter().find(|c| c.id == *cid).cloned();
        match conn {
            Some(c) => s.push_str(&format!(
                "- 连接：{}（{}，id `{cid}`）\n",
                c.name,
                c.kind.label()
            )),
            None => s.push_str(&format!("- 连接 id：`{cid}`\n")),
        }
    }
    if let Some(db) = &scope.database {
        s.push_str(&format!("- 数据库：{db}\n"));
    }
    if let Some(t) = &scope.table {
        s.push_str(&format!("- 当前表/集合/索引：{t}\n"));
    }
    Some(s)
}

// ---------- AI Chat with tools ----------

#[tauri::command]
pub async fn ai_chat(
    state: State<'_, AppState>,
    cache: State<'_, SharedCache>,
    session_id: String,
    user_message: String,
    profile_id: Option<String>,
    images: Option<Vec<String>>,
) -> Result<ChatSession, String> {
    let mut settings = resolve_ai_profile(&state, profile_id.as_deref())?;
    if settings.api_key.is_empty() {
        return Err(format!(
            "「{}」还没有配置 API Key，请到「设置 → 大模型」填写",
            settings.display_name()
        ));
    }
    // agent tasks need tools by default
    if looks_like_db_action(&user_message) {
        settings.enable_tools = true;
    }

    // 图片先校验再落盘，会话 JSON 里只留文件名
    let mut image_files: Vec<String> = Vec::new();
    for url in images.unwrap_or_default().iter() {
        let (mime, bytes) = parse_data_url(url).ok_or("图片不受支持：需要 image/png|jpeg|gif|webp|bmp 的 data URL")?;
        if bytes.len() > MAX_AI_IMAGE_BYTES {
            return Err(format!(
                "图片太大：{} MB，单张上限 {} MB",
                bytes.len() / (1024 * 1024),
                MAX_AI_IMAGE_BYTES / (1024 * 1024)
            ));
        }
        let name = format!("{}.{}", Uuid::new_v4().simple(), mime_ext(&mime));
        state.save_ai_image(&name, &bytes)?;
        image_files.push(name);
    }
    if image_files.len() > MAX_AI_IMAGES {
        state.remove_ai_images(&image_files);
        return Err(format!("一条消息最多 {MAX_AI_IMAGES} 张图片"));
    }

    // load / create session
    {
        let mut sessions = lock_or_recover(&state.sessions);
        if !sessions.iter().any(|s| s.id == session_id) {
            let title = user_message.chars().take(24).collect::<String>();
            sessions.insert(
                0,
                ChatSession {
                    id: session_id.clone(),
                    title,
                    messages: vec![],
                    created_at: now(),
                    updated_at: now(),
                    connection_id: None,
                    database: None,
                    table: None,
                },
            );
        }
    }

    let (history, scope) = {
        let sessions = lock_or_recover(&state.sessions);
        let s = sessions.iter().find(|s| s.id == session_id);
        (
            s.map(|s| s.messages.clone()).unwrap_or_default(),
            s.map(|s| s.scope()).unwrap_or_default(),
        )
    };

    // 历史里的图只重附最近几张，其余按文本继续
    let mut attach: std::collections::HashSet<String> = std::collections::HashSet::new();
    for m in history.iter().rev() {
        for n in m.images.iter().rev() {
            if attach.len() >= MAX_HISTORY_IMAGES {
                break;
            }
            attach.insert(n.clone());
        }
        if attach.len() >= MAX_HISTORY_IMAGES {
            break;
        }
    }

    let mut msgs: Vec<AiMessage> = Vec::new();
    for m in &history {
        let mut images = Vec::new();
        for n in &m.images {
            if !attach.contains(n) {
                continue;
            }
            match image_data_uri(&state, n) {
                Ok(u) => images.push(u),
                Err(e) => logger::warn("ai_chat", format!("历史图片丢失 {n}: {e}")),
            }
        }
        let mut text = m.content.clone();
        if !m.images.is_empty() && images.len() < m.images.len() {
            // 让模型知道这里曾经有图，只是没再传
            text.push_str(&format!("\n（本条消息含 {} 张图片，其中 {} 张已随本轮重新提供）", m.images.len(), images.len()));
        }
        msgs.push(AiMessage {
            role: m.role.clone(),
            content: text,
            images,
        });
    }
    msgs.push(AiMessage {
        role: "user".into(),
        content: user_message.clone(),
        images: image_files
            .iter()
            .filter_map(|n| image_data_uri(&state, n).ok())
            .collect(),
    });

    let facts = crate::ai::memory::list_fact_strings(&lock_or_recover(&state.memory));
    let (msgs, summary) = maybe_compress_history(&settings, &msgs, 12000).await?;
    let ctx = AgentContext {
        settings: settings.clone(),
        memory_facts: facts,
        compressed_summary: summary,
        scope_note: scope_note(&state, &scope),
    };
    let mut system = build_system_prompt(&ctx);
    system.push_str("\n\n## 实时上下文\n");
    system.push_str(&connection_snapshot(&state));
    system.push_str(
        "\n重要：涉及查库/查表/SQL/连接时，必须调用工具并基于工具结果作答，禁止只口头描述步骤。",
    );

    let mut chat_messages: Vec<AiMessage> = vec![AiMessage::text("system", system)];
    chat_messages.extend(msgs);

    let tools = if settings.enable_tools {
        Some(if settings.provider == "anthropic" {
            crate::ai::anthropic_tools_payload()
        } else {
            crate::ai::openai_tools_payload()
        })
    } else {
        None
    };

    let mut final_text = String::new();
    let mut tool_log = Vec::new();
    for round in 0..8 {
        // Force tool_calls on first DB-action turn (many gateways ignore auto)
        let force = round == 0 && looks_like_db_action(&user_message) && tools.is_some();
        let mut resp = crate::ai::chat_completion_opts(
            &settings,
            &chat_messages,
            tools.clone(),
            force,
        )
        .await
        .map_err(|e| {
            logger::error("ai_chat.completion", &e);
            e
        })?;

        // Fallback: model described tools in text instead of tool_calls
        if resp.tool_calls.is_empty() && tools.is_some() {
            let parsed = crate::ai::parse_tool_calls_from_text(&resp.content);
            if !parsed.is_empty() {
                logger::info(
                    "ai_chat",
                    format!("parsed {} tool calls from text", parsed.len()),
                );
                resp.tool_calls = parsed;
            }
        }

        if resp.tool_calls.is_empty() {
            final_text = resp.content;
            break;
        }
        let mut note = resp.content.clone();
        for tc in &resp.tool_calls {
            let result = execute_tool(&state, &cache, &tc.name, &tc.arguments, &scope)
                .await
                .unwrap_or_else(|e| json!({ "error": e }).to_string());
            tool_log.push(json!({
                "id": tc.id,
                "name": tc.name,
                "arguments": tc.arguments,
                "result_preview": result.chars().take(800).collect::<String>(),
            }));
            note.push_str(&format!(
                "\n[tool] {} => {}",
                tc.name,
                result.chars().take(400).collect::<String>()
            ));
            chat_messages.push(AiMessage::text(
                "tool",
                format!(
                    "工具 {} 执行结果:\n{}",
                    tc.name,
                    result.chars().take(4000).collect::<String>()
                ),
            ));
        }
        chat_messages.push(AiMessage::text("assistant", note));
        // After tools ran, ask model to summarize with force off
    }

    if final_text.is_empty() {
        final_text = "操作已执行，请查看工具日志。".into();
    }
    if !tool_log.is_empty() {
        final_text.push_str("\n\n---\n工具调用记录:\n");
        final_text.push_str(&serde_json::to_string_pretty(&tool_log).unwrap_or_default());
    }

    let tools_count = tool_log.len();
    let text_len = final_text.len();
    let image_count = image_files.len();

    // persist
    let session = {
        let mut sessions = lock_or_recover(&state.sessions);
        let s = sessions
            .iter_mut()
            .find(|s| s.id == session_id)
            .ok_or("会话不存在")?;
        s.messages.push(ChatMessage {
            id: Uuid::new_v4().to_string(),
            role: "user".into(),
            content: user_message,
            created_at: now(),
            tool_calls: None,
            images: image_files,
        });
        s.messages.push(ChatMessage {
            id: Uuid::new_v4().to_string(),
            role: "assistant".into(),
            content: final_text,
            created_at: now(),
            tool_calls: if tool_log.is_empty() {
                None
            } else {
                Some(Value::Array(tool_log))
            },
            images: vec![],
        });
        if s.title == "新对话" || s.title.is_empty() {
            s.title = s
                .messages
                .first()
                .map(|m| m.content.chars().take(24).collect::<String>())
                .unwrap_or_else(|| "对话".into());
        }
        s.updated_at = now();
        s.clone()
    };
    state.save_sessions();
    logger::info(
        "ai_chat",
        format!("session={session_id} tools={tools_count} images={image_count} chars={text_len}"),
    );
    Ok(session)
}

/// 单引号字面量转义（注释文本会拼进 DDL）
fn sql_str(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// 让模型给这张表补注释、给索引写说明并提建议；只返回结果，不写库
#[tauri::command]
pub async fn ai_enrich_table(
    state: State<'_, AppState>,
    cache: State<'_, SharedCache>,
    connection_id: String,
    database: String,
    table: String,
    schema: Option<String>,
    profile_id: Option<String>,
) -> Result<TableEnrichment, String> {
    let settings = resolve_ai_profile(&state, profile_id.as_deref())?;
    if settings.api_key.is_empty() {
        return Err(format!(
            "「{}」还没有配置 API Key，请到「设置 → 大模型」填写",
            settings.display_name()
        ));
    }
    let kind = kind_of(&state, &connection_id)?;
    if !matches!(
        kind,
        DbKind::Mysql | DbKind::Postgres | DbKind::Pgvector | DbKind::Sqlite
    ) {
        return Err("表结构补全目前用于 MySQL / PostgreSQL / SQLite".into());
    }
    let s = get_session(&cache, &state, &connection_id).await?;
    let columns = s.list_columns(&database, schema.as_deref(), &table).await?;
    let indexes = s
        .list_indexes(&database, schema.as_deref(), &table)
        .await
        .unwrap_or_default();
    let fks = s
        .list_foreign_keys(&database, schema.as_deref(), &table)
        .await
        .unwrap_or_default();
    let table_comment = s
        .list_tables(&database, schema.as_deref())
        .await
        .unwrap_or_default()
        .into_iter()
        .find(|t| t.name == table)
        .and_then(|t| t.comment);

    // 样例数据只用来判断业务含义，读不到也照样能补注释
    let sample_sql = match kind {
        DbKind::Mysql => format!("SELECT * FROM `{database}`.`{table}` LIMIT 5"),
        DbKind::Postgres | DbKind::Pgvector => {
            let t = match schema.as_deref() {
                Some(sc) if sc != "public" => format!("\"{sc}\".\"{table}\""),
                _ => format!("\"{table}\""),
            };
            format!("SELECT * FROM {t} LIMIT 5")
        }
        _ => format!("SELECT * FROM \"{table}\" LIMIT 5"),
    };
    let page = s
        .query_page(&database, &sample_sql, 1, 5)
        .await
        .unwrap_or_else(|_| QueryPage {
            columns: vec![],
            rows: vec![],
            total: 0,
            page: 1,
            page_size: 5,
            elapsed_ms: 0,
            message: None,
        });
    let header = page.columns.clone();
    let shape = crate::ai::enrich::TableShape {
        kind: kind.label(),
        database: &database,
        table: &table,
        table_comment: table_comment.as_deref(),
        columns: &columns,
        indexes: &indexes,
        fks: &fks,
        sample_header: &header,
        sample_rows: &page.rows,
    };
    let msgs = vec![AiMessage::text(
        "user",
        crate::ai::enrich::build_prompt(&shape),
    )];
    let reply = chat_completion(&settings, &msgs, None)
        .await
        .map_err(|e| {
            logger::error("ai_enrich_table.completion", &e);
            e
        })?;
    let names: Vec<String> = columns.iter().map(|c| c.name.clone()).collect();
    let out = crate::ai::enrich::parse_enrichment(&reply.content, &names, &indexes)
        .map_err(|e| log_err("ai_enrich_table.parse", &connection_id, &e))?;
    log_ok(
        "ai_enrich_table",
        &connection_id,
        &format!(
            "table={table} cols={} idx={} suggest={}",
            out.column_comments.len(),
            out.index_notes.len(),
            out.suggested_indexes.len()
        ),
    );
    Ok(out)
}

/// 把勾选的注释写回数据库：MySQL 要重述列定义，PostgreSQL 用 COMMENT ON
#[tauri::command]
pub async fn apply_table_comments(
    state: State<'_, AppState>,
    cache: State<'_, SharedCache>,
    connection_id: String,
    database: String,
    schema: Option<String>,
    table: String,
    table_comment: Option<String>,
    column_comments: Vec<ColumnComment>,
) -> Result<ExecResult, String> {
    let kind = kind_of(&state, &connection_id)?;
    let tc = table_comment.unwrap_or_default().trim().to_string();
    let cols: Vec<ColumnComment> = column_comments
        .into_iter()
        .filter(|c| !c.comment.trim().is_empty())
        .collect();
    if tc.is_empty() && cols.is_empty() {
        return Err("没有要写入的注释：请先勾选至少一条".into());
    }
    let s = get_session(&cache, &state, &connection_id).await?;
    let applied = match kind {
        DbKind::Mysql => {
            // 改列注释必须重述整个列定义，所以先读回 information_schema 的真实定义，
            // 否则 DEFAULT / AUTO_INCREMENT 会被悄悄抹掉
            let defs = s
                .query_page(
                    &database,
                    &format!(
                        "SELECT COLUMN_NAME, COLUMN_TYPE, IS_NULLABLE, COLUMN_DEFAULT, EXTRA \
                         FROM information_schema.COLUMNS WHERE TABLE_SCHEMA = {} AND TABLE_NAME = {}",
                        sql_str(&database),
                        sql_str(&table)
                    ),
                    1,
                    500,
                )
                .await?;
            let mut clauses: Vec<String> = Vec::new();
            if !tc.is_empty() {
                clauses.push(format!("COMMENT = {}", sql_str(&tc)));
            }
            for c in &cols {
                let Some(row) = defs.rows.iter().find(|r| {
                    r.first().and_then(|v| v.as_str()) == Some(c.name.as_str())
                }) else {
                    return Err(format!("列 {} 已经不存在，请刷新结构后重试", c.name));
                };
                let mut def = format!(
                    "`{}` {}",
                    c.name.replace('`', "``"),
                    row.get(1).and_then(|v| v.as_str()).unwrap_or("TEXT")
                );
                if row.get(2).and_then(|v| v.as_str()) == Some("NO") {
                    def.push_str(" NOT NULL");
                }
                match row.get(3) {
                    Some(Value::Null) | None => {}
                    Some(Value::String(d)) => {
                        let up = d.to_ascii_uppercase();
                        if up.starts_with("CURRENT_TIMESTAMP")
                            || up == "NULL"
                            || d.parse::<f64>().is_ok()
                        {
                            def.push_str(&format!(" DEFAULT {d}"));
                        } else {
                            def.push_str(&format!(" DEFAULT {}", sql_str(d)));
                        }
                    }
                    Some(other) => def.push_str(&format!(" DEFAULT {other}")),
                }
                if let Some(Value::String(e)) = row.get(4) {
                    if !e.trim().is_empty() {
                        def.push(' ');
                        def.push_str(e.trim());
                    }
                }
                def.push_str(&format!(" COMMENT {}", sql_str(c.comment.trim())));
                clauses.push(format!("MODIFY COLUMN {def}"));
            }
            let stmt = format!(
                "ALTER TABLE `{}`.`{}` {}",
                database.replace('`', "``"),
                table.replace('`', "``"),
                clauses.join(", ")
            );
            s.execute(&database, &stmt).await?;
            clauses.len()
        }
        DbKind::Postgres | DbKind::Pgvector => {
            let t = match schema.as_deref() {
                Some(sc) if sc != "public" => format!("\"{sc}\".\"{table}\""),
                _ => format!("\"{table}\""),
            };
            let mut n = 0usize;
            if !tc.is_empty() {
                s.execute(
                    &database,
                    &format!("COMMENT ON TABLE {t} IS {}", sql_str(&tc)),
                )
                .await?;
                n += 1;
            }
            for c in &cols {
                s.execute(
                    &database,
                    &format!(
                        "COMMENT ON COLUMN {}.\"{}\" IS {}",
                        t,
                        c.name.replace('"', "\"\""),
                        sql_str(c.comment.trim())
                    ),
                )
                .await?;
                n += 1;
            }
            n
        }
        _ => return Err("该引擎没有表 / 列注释能力，只能复制建议内容".into()),
    };
    let msg = format!("已写入 {applied} 条注释");
    log_ok(
        "apply_table_comments",
        &connection_id,
        &format!("table={table} {msg}"),
    );
    Ok(ExecResult {
        affected_rows: applied as u64,
        last_insert_id: None,
        message: Some(msg),
    })
}
#[tauri::command]
pub async fn test_ai_connection(settings: AiSettings) -> Result<String, String> {
    let msgs = vec![AiMessage::text("user", "请回复 OK")];
    chat_completion(&settings, &msgs, None)
        .await
        .map(|r| r.content)
        .map_err(|e| {
            logger::error("test_ai_connection", &e);
            e
        })
}

// ---------- Logging ----------

#[tauri::command]
pub fn log_frontend_error(
    message: String,
    stack: Option<String>,
    source: Option<String>,
) -> Result<(), String> {
    logger::log_frontend(crate::logger::FrontendError {
        message,
        stack,
        source,
    });
    Ok(())
}

#[tauri::command]
pub fn get_log_tail(max_lines: Option<usize>) -> String {
    logger::tail(max_lines.unwrap_or(200))
}

#[tauri::command]
pub fn open_log_dir(state: State<AppState>) -> Result<String, String> {
    let dir = state.data_dir.join("logs");
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.to_string_lossy().to_string();
    logger::info("open_log_dir", &path);
    Ok(path)
}

// ---------- Export / Dump / Sync ----------

fn kind_of(state: &AppState, connection_id: &str) -> Result<DbKind, String> {
    let list = lock_or_recover(&state.connections);
    list.iter()
        .find(|c| c.id == connection_id)
        .map(|c| c.kind.clone())
        .ok_or_else(|| format!("连接不存在: {connection_id}"))
}

#[tauri::command]
pub async fn export_query(
    state: State<'_, AppState>,
    cache: State<'_, SharedCache>,
    connection_id: String,
    database: String,
    sql: String,
    format: String,
    max_rows: Option<i64>,
    masks: Option<Vec<MaskRule>>,
) -> Result<ExportResult, String> {
    let masks = masks.unwrap_or_default();
    let kind = kind_of(&state, &connection_id)?;
    for m in &masks {
        if !crate::transfer::MASK_MODES.contains(&m.mode.as_str()) {
            return Err(format!("未知的脱敏规则：{}", m.mode));
        }
    }
    let s = get_session(&cache, &state, &connection_id).await?;
    let limit = crate::transfer::row_limit(max_rows, 50_000);
    let mut columns = Vec::new();
    let mut rows = Vec::new();
    let mut page = 1u64;
    loop {
        let budget = limit.saturating_sub(rows.len() as u64);
        if budget == 0 {
            break;
        }
        let p = s
            .query_page(&database, &sql, page, 1000.min(budget))
            .await
            .map_err(|e| log_err("export_query", &connection_id, &e))?;
        let n = p.rows.len();
        let total = p.total;
        columns = if columns.is_empty() {
            p.columns.clone()
        } else {
            columns
        };
        rows.extend(p.rows);
        if rows.len() as u64 >= limit || n == 0 {
            break;
        }
        if page * 1000 >= total && total > 0 {
            break;
        }
        page += 1;
        if limit == u64::MAX && page > 2_000_000 {
            break;
        }
        if limit != u64::MAX && page > limit / 1000 + 2 {
            break;
        }
    }
    if rows.len() as u64 > limit {
        rows.truncate(limit as usize);
    }
    let report = crate::transfer::apply_masks(&columns, &mut rows, &masks);
    let content = match format.as_str() {
        "excel" | "xls" | "xlsx" => crate::transfer::to_excel_xml(&columns, &rows),
        _ => crate::transfer::to_csv(&columns, &rows),
    };
    log_ok(
        "export_query",
        &connection_id,
        &format!(
            "fmt={format} rows={} mask={:?} unmatched={:?}",
            rows.len(),
            report.applied,
            report.unmatched
        ),
    );
    let _ = kind;
    Ok(ExportResult {
        content,
        rows: rows.len() as i64,
        masked_columns: report.applied,
        unmatched_columns: report.unmatched,
    })
}

#[tauri::command]
pub async fn dump_table(
    state: State<'_, AppState>,
    cache: State<'_, SharedCache>,
    connection_id: String,
    database: String,
    table: String,
    include_structure: bool,
    include_data: bool,
    max_rows: Option<i64>,
) -> Result<String, String> {
    let kind = kind_of(&state, &connection_id)?;
    if !matches!(
        kind,
        DbKind::Mysql | DbKind::Postgres | DbKind::Pgvector | DbKind::Sqlite
    ) {
        return Err("SQL 转储目前支持 MySQL / PostgreSQL / SQLite".into());
    }
    let s = get_session(&cache, &state, &connection_id).await?;
    let limit = crate::transfer::row_limit(max_rows, 100_000);
    let sql = crate::transfer::dump_table_sql(
        &s,
        kind,
        &database,
        &table,
        include_structure,
        include_data,
        limit,
    )
    .await
    .map_err(|e| log_err("dump_table", &connection_id, &e))?;
    log_ok(
        "dump_table",
        &connection_id,
        &format!(
            "table={table} bytes={} limit={}",
            sql.len(),
            crate::transfer::row_limit_label(limit)
        ),
    );
    Ok(sql)
}

#[tauri::command]
pub async fn dump_database(
    state: State<'_, AppState>,
    cache: State<'_, SharedCache>,
    connection_id: String,
    database: String,
    include_structure: bool,
    include_data: bool,
    max_rows: Option<i64>,
) -> Result<String, String> {
    let kind = kind_of(&state, &connection_id)?;
    if !matches!(
        kind,
        DbKind::Mysql | DbKind::Postgres | DbKind::Pgvector | DbKind::Sqlite
    ) {
        return Err("SQL 转储目前支持 MySQL / PostgreSQL / SQLite".into());
    }
    let s = get_session(&cache, &state, &connection_id).await?;
    let limit = crate::transfer::row_limit(max_rows, 50_000);
    let sql = crate::transfer::dump_database_sql(
        &s,
        kind,
        &database,
        include_structure,
        include_data,
        limit,
    )
    .await
    .map_err(|e| log_err("dump_database", &connection_id, &e))?;
    log_ok(
        "dump_database",
        &connection_id,
        &format!(
            "db={database} bytes={} limit={}",
            sql.len(),
            crate::transfer::row_limit_label(limit)
        ),
    );
    Ok(sql)
}

#[tauri::command]
pub async fn sync_table(
    state: State<'_, AppState>,
    cache: State<'_, SharedCache>,
    source_connection_id: String,
    source_database: String,
    source_table: String,
    target_connection_id: String,
    target_database: String,
    target_table: String,
    create_if_missing: bool,
    max_rows: Option<i64>,
) -> Result<SyncReport, String> {
    let src_kind = kind_of(&state, &source_connection_id)?;
    let dst_kind = kind_of(&state, &target_connection_id)?;
    let src = get_session(&cache, &state, &source_connection_id).await?;
    let dst = get_session(&cache, &state, &target_connection_id).await?;
    let limit = crate::transfer::row_limit(max_rows, 50_000);
    let one = crate::transfer::sync_table(
        &src,
        src_kind,
        &source_database,
        &source_table,
        &dst,
        dst_kind,
        &target_database,
        &target_table,
        create_if_missing,
        limit,
    )
    .await
    .map_err(|e| log_err("sync_table", &source_connection_id, &e))?;
    let report = crate::transfer::single_table_report(&target_database, one, dst_kind);
    log_ok(
        "sync_table",
        &source_connection_id,
        &format!(
            "{source_table} -> {target_table} limit={} : {}",
            crate::transfer::row_limit_label(limit),
            report.message
        ),
    );
    Ok(report)
}

#[tauri::command]
pub async fn sync_database(
    state: State<'_, AppState>,
    cache: State<'_, SharedCache>,
    source_connection_id: String,
    source_database: String,
    target_connection_id: String,
    target_database: String,
    create_if_missing: bool,
    max_rows: Option<i64>,
) -> Result<SyncReport, String> {
    let src_kind = kind_of(&state, &source_connection_id)?;
    let dst_kind = kind_of(&state, &target_connection_id)?;
    let src = get_session(&cache, &state, &source_connection_id).await?;
    let dst = get_session(&cache, &state, &target_connection_id).await?;
    let limit = crate::transfer::row_limit(max_rows, 20_000);
    let report = crate::transfer::sync_database(
        &src,
        src_kind,
        &source_database,
        &dst,
        dst_kind,
        &target_database,
        create_if_missing,
        limit,
    )
    .await
    .map_err(|e| log_err("sync_database", &source_connection_id, &e))?;
    log_ok(
        "sync_database",
        &source_connection_id,
        &format!(
            "{source_database} -> {target_database} limit={} : {}",
            crate::transfer::row_limit_label(limit),
            report.message
        ),
    );
    Ok(report)
}

#[tauri::command]
pub async fn write_text_file(path: String, content: String) -> Result<(), String> {
    if let Some(parent) = std::path::Path::new(&path).parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    std::fs::write(&path, content).map_err(|e| format!("写入文件失败: {e}"))
}

#[tauri::command]
pub async fn read_text_file(path: String) -> Result<String, String> {
    std::fs::read_to_string(&path).map_err(|e| format!("读取文件失败: {e}"))
}

/// Import CSV or SpreadsheetML (.xls) into a table.
#[tauri::command]
pub async fn truncate_table(
    state: State<'_, AppState>,
    cache: State<'_, SharedCache>,
    connection_id: String,
    database: String,
    table: String,
) -> Result<ExecResult, String> {
    let s = get_session(&cache, &state, &connection_id).await?;
    let res = s
        .truncate_table(&database, &table)
        .await
        .map_err(|e| log_err("truncate_table", &connection_id, &e))?;
    log_ok(
        "truncate_table",
        &connection_id,
        &format!("db={database} table={table} affected={}", res.affected_rows),
    );
    Ok(res)
}

#[tauri::command]
pub async fn drop_table(
    state: State<'_, AppState>,
    cache: State<'_, SharedCache>,
    connection_id: String,
    database: String,
    table: String,
) -> Result<ExecResult, String> {
    let s = get_session(&cache, &state, &connection_id).await?;
    let res = s
        .drop_table(&database, &table)
        .await
        .map_err(|e| log_err("drop_table", &connection_id, &e))?;
    log_ok("drop_table", &connection_id, &format!("db={database} table={table}"));
    Ok(res)
}

#[tauri::command]
pub async fn import_tabular_file(
    state: State<'_, AppState>,
    cache: State<'_, SharedCache>,
    connection_id: String,
    database: String,
    table: String,
    path: String,
    format: String,
    create_if_missing: bool,
) -> Result<String, String> {
    let kind = kind_of(&state, &connection_id)?;
    let text = std::fs::read_to_string(&path).map_err(|e| format!("读取文件失败: {e}"))?;
    let (header, rows) = if format == "excel" || path.to_ascii_lowercase().ends_with(".xls") {
        crate::transfer::parse_excel_xml(&text)
    } else {
        crate::transfer::parse_csv(&text)
    };
    if header.is_empty() {
        return Err("文件解析结果为空（请确认 CSV 有表头）".into());
    }
    let s = get_session(&cache, &state, &connection_id).await?;
    let res = crate::transfer::import_tabular(
        &s,
        kind,
        &database,
        &table,
        &header,
        &rows,
        create_if_missing,
    )
    .await
    .map_err(|e| log_err("import_tabular_file", &connection_id, &e))?;
    log_ok(
        "import_tabular_file",
        &connection_id,
        &format!("path={path} ok={} skip={}", res.inserted, res.skipped),
    );
    Ok(res.message)
}

/// Execute a .sql dump/script file statement by statement.
#[tauri::command]
pub async fn run_sql_script(
    state: State<'_, AppState>,
    cache: State<'_, SharedCache>,
    connection_id: String,
    database: String,
    path: String,
) -> Result<String, String> {
    let text = std::fs::read_to_string(&path).map_err(|e| format!("读取 SQL 文件失败: {e}"))?;
    let s = get_session(&cache, &state, &connection_id).await?;
    let res = crate::transfer::run_sql_script(&s, &database, &text)
        .await
        .map_err(|e| log_err("run_sql_script", &connection_id, &e))?;
    log_ok(
        "run_sql_script",
        &connection_id,
        &format!("path={path} ok={} fail={}", res.executed, res.failed),
    );
    Ok(res.message)
}

// ---------- 数据库设计（新建 / 编辑 / 删除库，PG 扩展） ----------

#[tauri::command]
pub async fn get_database_options(
    state: State<'_, AppState>,
    cache: State<'_, SharedCache>,
    connection_id: String,
    database: String,
) -> Result<DbOptions, String> {
    let s = get_session(&cache, &state, &connection_id)
        .await
        .map_err(|e| log_err("get_database_options.session", &connection_id, &e))?;
    s.db_options(&database)
        .await
        .map_err(|e| log_err("get_database_options", &connection_id, &e))
}

#[tauri::command]
pub async fn plan_database(
    state: State<'_, AppState>,
    cache: State<'_, SharedCache>,
    connection_id: String,
    action: String,
    spec: DbSpec,
) -> Result<Vec<String>, String> {
    let s = get_session(&cache, &state, &connection_id).await?;
    s.plan_database(&action, &spec)
}

#[tauri::command]
pub async fn apply_database(
    state: State<'_, AppState>,
    cache: State<'_, SharedCache>,
    connection_id: String,
    action: String,
    spec: DbSpec,
) -> Result<Vec<String>, String> {
    let s = get_session(&cache, &state, &connection_id)
        .await
        .map_err(|e| log_err("apply_database.session", &connection_id, &e))?;
    let stmts = if action == "drop" {
        s.drop_database(&spec.name).await
    } else {
        let planned = s.plan_database(&action, &spec)?;
        s.run_database_ddl(&planned).await.map(|_| planned)
    }
    .map_err(|e| log_err("apply_database", &connection_id, &e))?;
    log_ok(
        "apply_database",
        &connection_id,
        &format!("action={action} db={}", spec.name),
    );
    Ok(stmts)
}

#[tauri::command]
pub async fn list_extensions(
    state: State<'_, AppState>,
    cache: State<'_, SharedCache>,
    connection_id: String,
    database: String,
) -> Result<Vec<ExtInfo>, String> {
    let s = get_session(&cache, &state, &connection_id)
        .await
        .map_err(|e| log_err("list_extensions.session", &connection_id, &e))?;
    s.list_extensions(&database)
        .await
        .map_err(|e| log_err("list_extensions", &connection_id, &e))
}

#[tauri::command]
pub async fn apply_extension(
    state: State<'_, AppState>,
    cache: State<'_, SharedCache>,
    connection_id: String,
    database: String,
    action: String,
    name: String,
) -> Result<Vec<String>, String> {
    let s = get_session(&cache, &state, &connection_id)
        .await
        .map_err(|e| log_err("apply_extension.session", &connection_id, &e))?;
    let stmts = s.plan_extension(&action, &name)?;
    for st in &stmts {
        s.execute(&database, st)
            .await
            .map_err(|e| log_err("apply_extension", &connection_id, &e))?;
    }
    log_ok(
        "apply_extension",
        &connection_id,
        &format!("action={action} db={database} ext={name}"),
    );
    Ok(stmts)
}

// ---------- 可视化建表 ----------

#[tauri::command]
pub async fn plan_create_table(
    state: State<'_, AppState>,
    cache: State<'_, SharedCache>,
    connection_id: String,
    spec: TableSpec,
) -> Result<Vec<String>, String> {
    let s = get_session(&cache, &state, &connection_id).await?;
    s.plan_create_table(&spec)
}

#[tauri::command]
pub async fn apply_create_table(
    state: State<'_, AppState>,
    cache: State<'_, SharedCache>,
    connection_id: String,
    database: String,
    spec: TableSpec,
) -> Result<Vec<String>, String> {
    let s = get_session(&cache, &state, &connection_id)
        .await
        .map_err(|e| log_err("apply_create_table.session", &connection_id, &e))?;
    let stmts = s.plan_create_table(&spec)?;
    for st in &stmts {
        s.execute(&database, st)
            .await
            .map_err(|e| log_err("apply_create_table", &connection_id, &e))?;
    }
    log_ok(
        "apply_create_table",
        &connection_id,
        &format!("db={database} table={} stmts={}", spec.name, stmts.len()),
    );
    Ok(stmts)
}

// ---------- 数据库迁移 ----------

#[tauri::command]
pub async fn migrate_database(
    state: State<'_, AppState>,
    cache: State<'_, SharedCache>,
    source_connection_id: String,
    source_database: String,
    target_connection_id: String,
    target_database: String,
    options: MigrateOptions,
) -> Result<MigrateReport, String> {
    let src_kind = kind_of(&state, &source_connection_id)?;
    let dst_kind = kind_of(&state, &target_connection_id)?;
    let src = get_session(&cache, &state, &source_connection_id).await?;
    let dst = get_session(&cache, &state, &target_connection_id).await?;
    let report = crate::transfer::migrate_database(
        &src,
        src_kind,
        &source_database,
        &dst,
        dst_kind,
        &target_database,
        &options,
    )
    .await
    .map_err(|e| log_err("migrate_database", &source_connection_id, &e))?;
    log_ok(
        "migrate_database",
        &source_connection_id,
        &format!(
            "{source_database} -> {target_database} tables={} rows={} created={} skipped={} failed={}",
            report.tables.len(),
            report.total_rows,
            report.created,
            report.skipped,
            report.failed
        ),
    );
    Ok(report)
}
