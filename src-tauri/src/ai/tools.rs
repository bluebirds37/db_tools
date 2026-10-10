use crate::db;
use crate::store::{lock_or_recover, AppState};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// 会话缓存。锁只在同步读写 map 时持有，绝不跨 `await`，
/// 建会话按连接各自的门禁串行——某个库连不上时不会拖住其他库。
#[derive(Default)]
pub struct SessionCache {
    sessions: Mutex<HashMap<String, db::SharedSession>>,
    joining: Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
    failures: Mutex<HashMap<String, (std::time::Instant, String)>>,
}

/// 建连失败后的短冷却：同一连接的并发请求不再各自重付一次超时。
const FAIL_COOLDOWN: std::time::Duration = std::time::Duration::from_secs(3);

impl SessionCache {
    pub fn new() -> Self {
        Self::default()
    }

    fn cached(&self, connection_id: &str) -> Option<db::SharedSession> {
        lock_or_recover(&self.sessions).get(connection_id).cloned()
    }

    fn recent_failure(&self, connection_id: &str) -> Option<String> {
        let map = lock_or_recover(&self.failures);
        let (at, msg) = map.get(connection_id)?;
        if at.elapsed() < FAIL_COOLDOWN {
            return Some(msg.clone());
        }
        None
    }

    fn join_gate(&self, connection_id: &str) -> Arc<tokio::sync::Mutex<()>> {
        lock_or_recover(&self.joining)
            .entry(connection_id.to_string())
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
            .clone()
    }

    pub async fn get(
        &self,
        state: &AppState,
        connection_id: &str,
    ) -> Result<db::SharedSession, String> {
        if let Some(s) = self.cached(connection_id) {
            return Ok(s);
        }
        if let Some(msg) = self.recent_failure(connection_id) {
            return Err(msg);
        }
        let gate = self.join_gate(connection_id);
        let _joining = gate.lock().await;
        // 等待门禁期间别的调用可能已经建好会话
        if let Some(s) = self.cached(connection_id) {
            return Ok(s);
        }
        let config = {
            let list = lock_or_recover(&state.connections);
            list.iter()
                .find(|c| c.id == connection_id)
                .cloned()
                .ok_or_else(|| format!("连接不存在: {connection_id}"))?
        };
        match db::open_session(&config).await {
            Ok(session) => {
                lock_or_recover(&self.failures).remove(connection_id);
                lock_or_recover(&self.sessions)
                    .insert(connection_id.to_string(), session.clone());
                Ok(session)
            }
            Err(e) => {
                lock_or_recover(&self.failures)
                    .insert(connection_id.to_string(), (std::time::Instant::now(), e.clone()));
                Err(e)
            }
        }
    }

    pub fn invalidate(&self, connection_id: &str) {
        lock_or_recover(&self.sessions).remove(connection_id);
        lock_or_recover(&self.failures).remove(connection_id);
    }

    #[allow(dead_code)]
    pub fn clear(&self) {
        lock_or_recover(&self.sessions).clear();
        lock_or_recover(&self.failures).clear();
    }
}

pub type SharedCache = Arc<SessionCache>;

pub async fn get_session(
    cache: &SharedCache,
    state: &AppState,
    connection_id: &str,
) -> Result<db::SharedSession, String> {
    cache.get(state, connection_id).await
}

pub async fn execute_tool(
    state: &AppState,
    cache: &SharedCache,
    name: &str,
    args: &Value,
) -> Result<String, String> {
    let arg = |k: &str| args.get(k).cloned().unwrap_or(Value::Null);
    let arg_str = |k: &str| {
        args.get(k)
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_default()
    };

    match name {
        "list_connections" => {
            let list = lock_or_recover(&state.connections).clone();
            Ok(serde_json::to_string_pretty(&list).unwrap_or_default())
        }
        "list_databases" => {
            let cid = arg_str("connection_id");
            let s = get_session(cache, state, &cid).await?;
            let dbs = s.list_databases().await?;
            Ok(json!(dbs).to_string())
        }
        "list_tables" => {
            let cid = arg_str("connection_id");
            let database = arg_str("database");
            let s = get_session(cache, state, &cid).await?;
            let tables = s.list_tables(&database, None).await?;
            Ok(serde_json::to_string_pretty(&tables).unwrap_or_default())
        }
        "describe_table" => {
            let cid = arg_str("connection_id");
            let database = arg_str("database");
            let table = arg_str("table");
            let s = get_session(cache, state, &cid).await?;
            let cols = s.list_columns(&database, None, &table).await?;
            let idx = s.list_indexes(&database, None, &table).await.unwrap_or_default();
            let fks = s
                .list_foreign_keys(&database, None, &table)
                .await
                .unwrap_or_default();
            Ok(json!({
                "columns": cols,
                "indexes": idx,
                "foreign_keys": fks
            })
            .to_string())
        }
        "query_data" => {
            let cid = arg_str("connection_id");
            let database = arg_str("database");
            let sql = arg_str("sql");
            let page = arg("page").as_u64().unwrap_or(1).max(1);
            let page_size = arg("page_size").as_u64().unwrap_or(20).clamp(1, 200);
            let s = get_session(cache, state, &cid).await?;
            let page_res = s.query_page(&database, &sql, page, page_size).await?;
            // trim large payloads for AI
            let mut compact = page_res.clone();
            if compact.rows.len() > 20 {
                compact.rows.truncate(20);
            }
            for row in compact.rows.iter_mut() {
                for cell in row.iter_mut() {
                    if let Value::String(st) = cell {
                        if st.len() > 300 {
                            *cell = Value::String(format!("{}…", &st[..300]));
                        }
                    }
                }
            }
            Ok(serde_json::to_string(&compact).unwrap_or_default())
        }
        "execute_sql" => {
            let cid = arg_str("connection_id");
            let database = arg_str("database");
            let sql = arg_str("sql");
            let s = get_session(cache, state, &cid).await?;
            let res = s.execute(&database, &sql).await?;
            Ok(serde_json::to_string(&res).unwrap_or_default())
        }
        "insert_row" => {
            let cid = arg_str("connection_id");
            let database = arg_str("database");
            let table = arg_str("table");
            let data = arg("data").as_object().cloned().unwrap_or_default();
            let s = get_session(cache, state, &cid).await?;
            let res = s.insert_row(&database, &table, data).await?;
            Ok(serde_json::to_string(&res).unwrap_or_default())
        }
        "update_row" => {
            let cid = arg_str("connection_id");
            let database = arg_str("database");
            let table = arg_str("table");
            let pk = arg("pk").as_object().cloned().unwrap_or_default();
            let data = arg("data").as_object().cloned().unwrap_or_default();
            let s = get_session(cache, state, &cid).await?;
            let res = s.update_row(&database, &table, pk, data).await?;
            Ok(serde_json::to_string(&res).unwrap_or_default())
        }
        "delete_row" => {
            let cid = arg_str("connection_id");
            let database = arg_str("database");
            let table = arg_str("table");
            let pk = arg("pk").as_object().cloned().unwrap_or_default();
            let s = get_session(cache, state, &cid).await?;
            let res = s.delete_row(&database, &table, pk).await?;
            Ok(serde_json::to_string(&res).unwrap_or_default())
        }
        "remember_fact" => {
            let content = arg_str("content");
            crate::ai::memory::add_fact(&mut lock_or_recover(&state.memory), &content);
            state.save_memory();
            Ok(json!({ "ok": true, "content": content }).to_string())
        }
        other => Err(format!("未知工具: {other}")),
    }
}
