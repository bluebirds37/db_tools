use crate::db;
use crate::models::ChatScope;
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

/// 工具参数优先，缺省回落到会话范围；空字符串按没填处理
pub fn scope_arg(args: &Value, key: &str, fallback: &Option<String>) -> String {
    let given = args
        .get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    given
        .or_else(|| {
            fallback
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
        })
        .unwrap_or_default()
}

/// 模型省略 connection_id / database / table 时按会话已选范围补齐：
/// 面板选好库后不必再为拿 ID 先绕一圈 list_connections。
pub async fn execute_tool(
    state: &AppState,
    cache: &SharedCache,
    name: &str,
    args: &Value,
    scope: &ChatScope,
) -> Result<String, String> {
    let arg = |k: &str| args.get(k).cloned().unwrap_or(Value::Null);
    let pick = |k: &str, fallback: &Option<String>| scope_arg(args, k, fallback);
    let cid = pick("connection_id", &scope.connection_id);
    let database = pick("database", &scope.database);
    let table = pick("table", &scope.table);

    // 只有这两个工具不需要连接
    if cid.is_empty() && name != "list_connections" && name != "remember_fact" {
        return Err(
            "尚未确定连接：请在智能体面板选好连接与库，或先调用 list_connections 后带上 connection_id"
                .to_string(),
        );
    }

    match name {
        "list_connections" => {
            let list = lock_or_recover(&state.connections).clone();
            Ok(serde_json::to_string_pretty(&list).unwrap_or_default())
        }
        "list_databases" => {
            let s = get_session(cache, state, &cid).await?;
            let dbs = s.list_databases().await?;
            Ok(json!(dbs).to_string())
        }
        "list_tables" => {
            let s = get_session(cache, state, &cid).await?;
            let tables = s.list_tables(&database, None).await?;
            Ok(serde_json::to_string_pretty(&tables).unwrap_or_default())
        }
        "describe_table" => {
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
            let sql = pick("sql", &None);
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
            let sql = pick("sql", &None);
            let s = get_session(cache, state, &cid).await?;
            let res = s.execute(&database, &sql).await?;
            Ok(serde_json::to_string(&res).unwrap_or_default())
        }
        "insert_row" => {
            let data = arg("data").as_object().cloned().unwrap_or_default();
            let s = get_session(cache, state, &cid).await?;
            let res = s.insert_row(&database, &table, data).await?;
            Ok(serde_json::to_string(&res).unwrap_or_default())
        }
        "update_row" => {
            let pk = arg("pk").as_object().cloned().unwrap_or_default();
            let data = arg("data").as_object().cloned().unwrap_or_default();
            let s = get_session(cache, state, &cid).await?;
            let res = s.update_row(&database, &table, pk, data).await?;
            Ok(serde_json::to_string(&res).unwrap_or_default())
        }
        "delete_row" => {
            let pk = arg("pk").as_object().cloned().unwrap_or_default();
            let s = get_session(cache, state, &cid).await?;
            let res = s.delete_row(&database, &table, pk).await?;
            Ok(serde_json::to_string(&res).unwrap_or_default())
        }
        "remember_fact" => {
            let content = pick("content", &None);
            crate::ai::memory::add_fact(&mut lock_or_recover(&state.memory), &content);
            state.save_memory();
            Ok(json!({ "ok": true, "content": content }).to_string())
        }
        other => Err(format!("未知工具: {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn explicit_arg_beats_session_scope() {
        let scope = ChatScope {
            connection_id: Some("c1".into()),
            database: Some("db1".into()),
            table: None,
        };
        let args = json!({ "connection_id": "c2", "database": "  " });
        assert_eq!(scope_arg(&args, "connection_id", &scope.connection_id), "c2");
        // 空白按没填处理，回落到会话
        assert_eq!(scope_arg(&args, "database", &scope.database), "db1");
        assert_eq!(scope_arg(&args, "table", &scope.table), "");
    }

    #[test]
    fn numeric_args_still_readable() {
        let args = json!({ "sql": "select 1", "page": 3 });
        assert_eq!(scope_arg(&args, "sql", &None), "select 1");
        assert_eq!(args.get("page").and_then(|v| v.as_u64()), Some(3));
    }
}
