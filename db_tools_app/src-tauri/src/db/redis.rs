use super::*;
use crate::models::ConnectionConfig;
use ::redis::{cmd, AsyncCommands, Client, Value as RedisValue};

pub struct RedisSession {
    client: Client,
    /// Connection URL without the trailing logical DB number.
    base_url: String,
    db_index: i64,
    connect_timeout: std::time::Duration,
    clients: std::sync::Mutex<std::collections::HashMap<i64, Client>>,
}

impl RedisSession {
    pub async fn connect(config: &ConnectionConfig) -> Result<Self, String> {
        let host = config.host.clone().unwrap_or_else(|| "127.0.0.1".into());
        let port = config.port.unwrap_or(6379);
        let db_index: i64 = config
            .database
            .clone()
            .or_else(|| config.options.get("db").cloned())
            .and_then(|s| Self::parse_db_index(&s))
            .unwrap_or(0);
        let mut url = format!("redis://");
        if let Some(user) = config.username.as_ref().filter(|u| !u.is_empty()) {
            url.push_str(user);
            url.push('@');
        }
        if let Some(pass) = config.password.as_ref().filter(|p| !p.is_empty()) {
            if config.username.as_ref().filter(|u| !u.is_empty()).is_none() {
                url.push(':');
            }
            url.push_str(pass);
            url.push('@');
        }
        let base_url = format!("{}{}:{}", url, host, port);
        let connect_timeout = connect_timeout(config);
        let client = Client::open(format!("{base_url}/{db_index}"))
            .map_err(|e| format!("Redis 客户端错误: {e}"))?;
        let mut con = Self::with_timeout(
            connect_timeout,
            client.get_multiplexed_async_connection(),
            "Redis 连接失败",
        )
        .await?;
        let _: String = Self::with_timeout(
            connect_timeout,
            cmd("PING").query_async(&mut con),
            "Redis PING 失败",
        )
        .await?;
        Ok(Self {
            client,
            base_url,
            db_index,
            connect_timeout,
            clients: std::sync::Mutex::new(std::collections::HashMap::new()),
        })
    }

    /// 给没有内建时限的 Redis 操作套一层超时。
    async fn with_timeout<T, E: std::fmt::Display>(
        limit: std::time::Duration,
        fut: impl std::future::Future<Output = Result<T, E>>,
        label: &str,
    ) -> Result<T, String> {
        tokio::time::timeout(limit, fut)
            .await
            .map_err(|_| format!("{label}：超过 {}s 无响应", limit.as_secs()))?
            .map_err(|e| format!("{label}: {e}"))
    }

    fn parse_db_index(s: &str) -> Option<i64> {
        s.trim()
            .strip_prefix("db")
            .or(Some(s.trim()))
            .and_then(|t| t.trim().parse::<i64>().ok())
    }

    /// Logical DB an operation targets; unknown names fall back to the connection default.
    fn db_index_of(&self, database: &str) -> i64 {
        Self::parse_db_index(database).unwrap_or(self.db_index)
    }

    fn client_for(&self, idx: i64) -> Result<Client, String> {
        if idx == self.db_index {
            return Ok(self.client.clone());
        }
        let mut map = self
            .clients
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(c) = map.get(&idx) {
            return Ok(c.clone());
        }
        let client = Client::open(format!("{}/{}", self.base_url, idx))
            .map_err(|e| format!("Redis 客户端错误: {e}"))?;
        map.insert(idx, client.clone());
        Ok(client)
    }

    async fn conn(
        &self,
        database: &str,
    ) -> Result<::redis::aio::MultiplexedConnection, String> {
        let idx = self.db_index_of(database);
        let client = self.client_for(idx)?;
        Self::with_timeout(
            self.connect_timeout,
            client.get_multiplexed_async_connection(),
            "Redis 连接失败",
        )
        .await
    }
}

#[async_trait]
impl DbSession for RedisSession {
    fn kind(&self) -> DbKind {
        DbKind::Redis
    }

    async fn ping(&self) -> Result<(), String> {
        let mut con = self.conn("").await?;
        let _: String = cmd("PING").query_async(&mut con).await.map_err(|e| e.to_string())?;
        Ok(())
    }

    async fn list_databases(&self) -> Result<Vec<String>, String> {
        // Redis logical DBs 0-15
        Ok((0..16).map(|i| format!("db{i}")).collect())
    }

    async fn list_tables(&self, database: &str, _schema: Option<&str>) -> Result<Vec<TableInfo>, String> {
        // Represent as key-type groups
        let mut con = self.conn(database).await?;
        let keys: Vec<String> = cmd("KEYS")
            .arg("*")
            .query_async(&mut con)
            .await
            .unwrap_or_default();
        let mut groups: std::collections::BTreeMap<String, i64> = Default::default();
        let sample: Vec<String> = keys.into_iter().take(2000).collect();
        for k in &sample {
            let t: String = cmd("TYPE")
                .arg(k)
                .query_async(&mut con)
                .await
                .unwrap_or_else(|_| "unknown".into());
            *groups.entry(t).or_default() += 1;
        }
        Ok(groups
            .into_iter()
            .map(|(t, n)| TableInfo {
                name: t.clone(),
                schema: None,
                table_type: Some("KEY_TYPE".into()),
                comment: Some(format!("{n} 个 key")),
                row_estimate: Some(n),
                tablespace: None,
            })
            .collect())
    }

    async fn list_columns(&self, _database: &str, _schema: Option<&str>, table: &str) -> Result<Vec<ColumnInfo>, String> {
        // table is key type e.g. string/hash/list/set/zset
        Ok(vec![
            ColumnInfo {
                name: "key".into(),
                data_type: "string".into(),
                nullable: false,
                primary_key: true,
                default: None,
                comment: Some("Redis key".into()),
                extra: None,
                max_length: None,
            },
            ColumnInfo {
                name: "type".into(),
                data_type: format!("const:{table}"),
                nullable: false,
                primary_key: false,
                default: None,
                comment: None,
                extra: None,
                max_length: None,
            },
            ColumnInfo {
                name: "ttl".into(),
                data_type: "int".into(),
                nullable: true,
                primary_key: false,
                default: None,
                comment: Some("-1 永不过期".into()),
                extra: None,
                max_length: None,
            },
            ColumnInfo {
                name: "value".into(),
                data_type: "string".into(),
                nullable: true,
                primary_key: false,
                default: None,
                comment: None,
                extra: None,
                max_length: None,
            },
        ])
    }

    async fn list_indexes(&self, _database: &str, _schema: Option<&str>, _table: &str) -> Result<Vec<IndexInfo>, String> {
        Ok(vec![IndexInfo {
            name: "PRIMARY".into(),
            columns: vec!["key".into()],
            unique: true,
            primary: true,
        }])
    }

    async fn list_foreign_keys(
        &self,
        _database: &str,
        _schema: Option<&str>,
        _table: &str,
    ) -> Result<Vec<ForeignKeyInfo>, String> {
        Ok(vec![])
    }

    async fn query_page(
        &self,
        database: &str,
        sql: &str,
        page: u64,
        page_size: u64,
    ) -> Result<QueryPage, String> {
        let start = std::time::Instant::now();
        let mut con = self.conn(database).await?;
        let pattern = parse_redis_pattern(sql);
        // 左侧「表」是键类型分组，点开分组时按类型筛键而不是把分组名当 pattern
        let type_group = matches!(
            pattern.as_str(),
            "string" | "hash" | "list" | "set" | "zset" | "stream"
        );
        let keys: Vec<String> = if type_group {
            let all: Vec<String> = cmd("KEYS")
                .arg("*")
                .query_async(&mut con)
                .await
                .unwrap_or_default();
            let mut matched = Vec::new();
            for k in all.into_iter().take(3000) {
                let t: String = cmd("TYPE")
                    .arg(&k)
                    .query_async(&mut con)
                    .await
                    .unwrap_or_default();
                if t == pattern {
                    matched.push(k);
                }
            }
            matched
        } else {
            cmd("KEYS")
                .arg(&pattern)
                .query_async(&mut con)
                .await
                .map_err(|e| e.to_string())?
        };
        let total = keys.len() as u64;
        let offset = page.saturating_sub(1) * page_size;
        let page_keys: Vec<String> = keys
            .into_iter()
            .skip(offset as usize)
            .take(page_size as usize)
            .collect();

        let mut rows = Vec::new();
        for k in &page_keys {
            let t: String = cmd("TYPE")
                .arg(k)
                .query_async(&mut con)
                .await
                .unwrap_or_else(|_| "unknown".into());
            let ttl: i64 = cmd("TTL")
                .arg(k)
                .query_async(&mut con)
                .await
                .unwrap_or(-2);
            let value: String = match t.as_str() {
                "string" => cmd("GET")
                    .arg(k)
                    .query_async(&mut con)
                    .await
                    .unwrap_or_default(),
                "hash" => {
                    let h: std::collections::HashMap<String, String> = cmd("HGETALL")
                        .arg(k)
                        .query_async(&mut con)
                        .await
                        .unwrap_or_default();
                    serde_json::to_string(&h).unwrap_or_default()
                }
                "list" => {
                    let l: Vec<String> = cmd("LRANGE")
                        .arg(k)
                        .arg(0)
                        .arg(20)
                        .query_async(&mut con)
                        .await
                        .unwrap_or_default();
                    serde_json::to_string(&l).unwrap_or_default()
                }
                "set" => {
                    let s: Vec<String> = cmd("SMEMBERS")
                        .arg(k)
                        .query_async(&mut con)
                        .await
                        .unwrap_or_default();
                    serde_json::to_string(&s.iter().take(20).collect::<Vec<_>>())
                        .unwrap_or_default()
                }
                "zset" => {
                    let z: Vec<String> = cmd("ZRANGE")
                        .arg(k)
                        .arg(0)
                        .arg(20)
                        .arg("WITHSCORES")
                        .query_async(&mut con)
                        .await
                        .unwrap_or_default();
                    serde_json::to_string(&z).unwrap_or_default()
                }
                other => format!("(type={other})"),
            };
            rows.push(vec![
                serde_json::Value::String(k.clone()),
                serde_json::Value::String(t),
                serde_json::Value::from(ttl),
                serde_json::Value::String(value),
            ]);
        }

        Ok(QueryPage {
            columns: vec!["key".into(), "type".into(), "ttl".into(), "value".into()],
            rows,
            total,
            page,
            page_size,
            elapsed_ms: start.elapsed().as_millis() as u64,
            message: Some(format!(
                "{}: {pattern}",
                if type_group { "按类型筛选" } else { "匹配 pattern" }
            )),
        })
    }

    async fn execute(&self, database: &str, sql: &str) -> Result<ExecResult, String> {
        let mut con = self.conn(database).await?;
        let parts: Vec<&str> = sql.split_whitespace().collect();
        if parts.is_empty() {
            return Err("空命令".into());
        }
        let mut cmd = cmd(parts[0]);
        for p in &parts[1..] {
            cmd.arg(*p);
        }
        let result: RedisValue = cmd.query_async(&mut con).await.map_err(|e| e.to_string())?;
        Ok(ExecResult {
            affected_rows: 1,
            last_insert_id: None,
            message: Some(render_redis_value(&result)),
        })
    }

    async fn insert_row(
        &self,
        database: &str,
        table: &str,
        data: serde_json::Map<String, serde_json::Value>,
    ) -> Result<ExecResult, String> {
        let key = data
            .get("key")
            .and_then(|v| v.as_str())
            .ok_or("缺少 key 字段")?;
        let value = data
            .get("value")
            .map(|v| match v {
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string(),
            })
            .unwrap_or_default();
        let mut con = self.conn(database).await?;
        match table {
            "string" | "" => {
                let _: () = con.set(key, value).await.map_err(|e| e.to_string())?;
            }
            "hash" => {
                let map: serde_json::Map<String, serde_json::Value> =
                    serde_json::from_str(&value).unwrap_or_default();
                for (k, v) in map {
                    let _: () = cmd("HSET")
                        .arg(key)
                        .arg(k)
                        .arg(v.to_string())
                        .query_async(&mut con)
                        .await
                        .map_err(|e| e.to_string())?;
                }
            }
            "list" => {
                let _: () = con.rpush(key, value).await.map_err(|e| e.to_string())?;
            }
            "set" => {
                let _: () = con.sadd(key, value).await.map_err(|e| e.to_string())?;
            }
            "zset" => {
                let _: () = cmd("ZADD")
                    .arg(key)
                    .arg(0)
                    .arg(value)
                    .query_async(&mut con)
                    .await
                    .map_err(|e| e.to_string())?;
            }
            other => return Err(format!("不支持的类型: {other}")),
        }
        Ok(ExecResult {
            affected_rows: 1,
            last_insert_id: None,
            message: Some(format!("已写入 key: {key}")),
        })
    }

    async fn update_row(
        &self,
        database: &str,
        _table: &str,
        pk: serde_json::Map<String, serde_json::Value>,
        data: serde_json::Map<String, serde_json::Value>,
    ) -> Result<ExecResult, String> {
        let mut merged = pk;
        for (k, v) in data {
            merged.insert(k, v);
        }
        self.insert_row(database, "string", merged).await
    }

    async fn delete_row(
        &self,
        database: &str,
        _table: &str,
        pk: serde_json::Map<String, serde_json::Value>,
    ) -> Result<ExecResult, String> {
        let key = pk
            .get("key")
            .and_then(|v| v.as_str())
            .ok_or("缺少 key 字段")?;
        let mut con = self.conn(database).await?;
        let n: i64 = con.del(key).await.map_err(|e| e.to_string())?;
        Ok(ExecResult {
            affected_rows: n as u64,
            last_insert_id: None,
            message: Some(format!("删除 {n} 个 key")),
        })
    }
}

/// 命令回显：把 Redis 回复渲染成可读文本，而不是 Rust 的 Debug 结构。
fn render_redis_value(v: &RedisValue) -> String {
    match v {
        RedisValue::Nil => "(nil)".into(),
        RedisValue::Okay => "OK".into(),
        RedisValue::SimpleString(s) => s.clone(),
        RedisValue::BulkString(b) => String::from_utf8_lossy(b).into_owned(),
        RedisValue::VerbatimString { text, .. } => text.clone(),
        RedisValue::Int(i) => i.to_string(),
        RedisValue::Double(d) => d.to_string(),
        RedisValue::Boolean(b) => b.to_string(),
        RedisValue::Array(items) | RedisValue::Set(items) => items
            .iter()
            .enumerate()
            .map(|(i, item)| format!("{}) {}", i + 1, render_redis_value(item)))
            .collect::<Vec<_>>()
            .join("\n"),
        RedisValue::Map(pairs) => pairs
            .iter()
            .map(|(k, val)| format!("{}: {}", render_redis_value(k), render_redis_value(val)))
            .collect::<Vec<_>>()
            .join("\n"),
        other => format!("{other:?}"),
    }
}

fn parse_redis_pattern(sql: &str) -> String {
    let s = sql.trim();
    if s.is_empty() {
        return "*".into();
    }
    // support: KEYS pattern  or raw pattern
    let upper = s.to_uppercase();
    if upper.starts_with("KEYS ") {
        s[5..].trim().to_string()
    } else if upper.starts_with("SCAN ") {
        "*".into()
    } else {
        s.to_string()
    }
}
