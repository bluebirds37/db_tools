pub mod elasticsearch;
pub mod mongodb;
pub mod mysql;
pub mod postgres;
pub mod qdrant;
pub mod redis;
pub mod sqlite;

use crate::models::*;
use async_trait::async_trait;
use std::sync::Arc;

#[async_trait]
pub trait DbSession: Send + Sync {
    #[allow(dead_code)]
    fn kind(&self) -> DbKind;
    async fn ping(&self) -> Result<(), String>;
    async fn list_databases(&self) -> Result<Vec<String>, String>;
    /// PostgreSQL schemas; empty for engines without schemas.
    async fn list_schemas(&self, _database: &str) -> Result<Vec<String>, String> {
        Ok(vec![])
    }
    async fn list_tables(
        &self,
        database: &str,
        schema: Option<&str>,
    ) -> Result<Vec<TableInfo>, String>;
    async fn list_columns(
        &self,
        database: &str,
        schema: Option<&str>,
        table: &str,
    ) -> Result<Vec<ColumnInfo>, String>;
    async fn list_indexes(
        &self,
        database: &str,
        schema: Option<&str>,
        table: &str,
    ) -> Result<Vec<IndexInfo>, String>;
    async fn list_foreign_keys(
        &self,
        database: &str,
        schema: Option<&str>,
        table: &str,
    ) -> Result<Vec<ForeignKeyInfo>, String>;

    /// PostgreSQL tablespaces; empty for engines without the concept.
    async fn list_tablespaces(&self) -> Result<Vec<TablespaceInfo>, String> {
        Ok(vec![])
    }

    /// Schema / tablespace / size metadata for one table.
    async fn describe_table_extra(
        &self,
        _database: &str,
        _schema: Option<&str>,
        _table: &str,
    ) -> Result<TableExtra, String> {
        Ok(TableExtra {
            schema: None,
            tablespace: None,
            size: None,
        })
    }
    async fn query_page(
        &self,
        database: &str,
        sql: &str,
        page: u64,
        page_size: u64,
    ) -> Result<QueryPage, String>;
    async fn execute(&self, database: &str, sql: &str) -> Result<ExecResult, String>;
    async fn insert_row(
        &self,
        database: &str,
        table: &str,
        data: serde_json::Map<String, serde_json::Value>,
    ) -> Result<ExecResult, String>;
    async fn update_row(
        &self,
        database: &str,
        table: &str,
        pk: serde_json::Map<String, serde_json::Value>,
        data: serde_json::Map<String, serde_json::Value>,
    ) -> Result<ExecResult, String>;
    async fn delete_row(
        &self,
        database: &str,
        table: &str,
        pk: serde_json::Map<String, serde_json::Value>,
    ) -> Result<ExecResult, String>;

    // ---------- 数据库设计（仅部分引擎支持） ----------

    fn db_caps(&self) -> DbCaps {
        DbCaps::default()
    }

    /// 设计器需要的候选值（角色 / 模板 / 编码 / 排序规则 / 表空间…）
    async fn db_options(&self, _database: &str) -> Result<DbOptions, String> {
        Err(format!("{} 不支持数据库设计", self.kind().label()))
    }

    /// 只生成 DDL：SQL 预览与实际执行共用同一个来源，预览所见即所执行
    fn plan_database(&self, _action: &str, _spec: &DbSpec) -> Result<Vec<String>, String> {
        Err(format!("{} 不支持数据库设计", self.kind().label()))
    }

    async fn run_database_ddl(&self, stmts: &[String]) -> Result<(), String> {
        for s in stmts {
            self.execute("", s).await?;
        }
        Ok(())
    }

    /// 执行删库；引擎可在这里处理自己特有的阻碍（如 PG 的活跃连接）
    async fn drop_database(&self, name: &str) -> Result<Vec<String>, String> {
        let spec = DbSpec {
            name: name.to_string(),
            ..Default::default()
        };
        let stmts = self.plan_database("drop", &spec)?;
        self.run_database_ddl(&stmts).await?;
        Ok(stmts)
    }

    async fn list_extensions(&self, _database: &str) -> Result<Vec<ExtInfo>, String> {
        Err(format!("{} 不支持扩展管理", self.kind().label()))
    }

    fn plan_extension(&self, _action: &str, _name: &str) -> Result<Vec<String>, String> {
        Err(format!("{} 不支持扩展管理", self.kind().label()))
    }

    /// 可视化建表的 DDL；只有关系型引擎支持
    fn plan_create_table(&self, _spec: &TableSpec) -> Result<Vec<String>, String> {
        Err(format!("{} 不支持可视化建表", self.kind().label()))
    }
}

/// 建表前的统一校验，返回 (有效列, 主键列)
pub fn validate_table_spec(
    spec: &TableSpec,
) -> Result<(Vec<&ColSpec>, Vec<String>), String> {
    use std::collections::HashSet;
    let name = spec.name.trim();
    if name.is_empty() {
        return Err("表名不能为空".into());
    }
    let cols: Vec<&ColSpec> = spec
        .columns
        .iter()
        .filter(|c| !c.name.trim().is_empty())
        .collect();
    if cols.is_empty() {
        return Err("至少需要一个字段".into());
    }
    let mut seen: HashSet<String> = HashSet::new();
    for c in &cols {
        let n = c.name.trim();
        if c.data_type.as_deref().unwrap_or("").trim().is_empty() {
            return Err(format!("字段 {n} 缺少类型"));
        }
        if !seen.insert(n.to_ascii_lowercase()) {
            return Err(format!("字段名重复: {n}"));
        }
    }
    let pks: Vec<String> = cols
        .iter()
        .filter(|c| c.primary_key.unwrap_or(false))
        .map(|c| c.name.trim().to_string())
        .collect();
    Ok((cols, pks))
}

pub type SharedSession = Arc<dyn DbSession>;

/// 建连超时：默认 6 秒，可用连接的 `connect_timeout` 选项（秒）放宽或收紧。
/// 时限只在这里定义，各驱动都从这里取值。
pub fn connect_timeout(config: &ConnectionConfig) -> std::time::Duration {
    config
        .options
        .get("connect_timeout")
        .and_then(|s| s.trim().parse::<f64>().ok())
        .filter(|secs| *secs >= 1.0 && *secs <= 120.0)
        .map(|secs| std::time::Duration::from_secs(secs as u64))
        .unwrap_or_else(|| std::time::Duration::from_secs(6))
}

pub async fn open_session(config: &ConnectionConfig) -> Result<SharedSession, String> {
    let limit = connect_timeout(config);
    let opened = open_session_inner(config);
    tokio::time::timeout(limit, opened)
        .await
        .map_err(|_| {
            format!(
                "连接超时（{}s）：请确认地址、端口与服务状态；可在连接的 connect_timeout 选项放宽",
                limit.as_secs()
            )
        })?
}

async fn open_session_inner(config: &ConnectionConfig) -> Result<SharedSession, String> {
    match config.kind {
        DbKind::Mysql => mysql::MySqlSession::connect(config)
            .await
            .map(|s| Arc::new(s) as SharedSession),
        DbKind::Postgres | DbKind::Pgvector => postgres::PostgresSession::connect(config)
            .await
            .map(|s| Arc::new(s) as SharedSession),
        DbKind::Sqlite => sqlite::SqliteSession::connect(config)
            .await
            .map(|s| Arc::new(s) as SharedSession),
        DbKind::Redis => redis::RedisSession::connect(config)
            .await
            .map(|s| Arc::new(s) as SharedSession),
        DbKind::Mongodb => mongodb::MongoSession::connect(config)
            .await
            .map(|s| Arc::new(s) as SharedSession),
        DbKind::Elasticsearch => elasticsearch::EsSession::connect(config)
            .await
            .map(|s| Arc::new(s) as SharedSession),
        DbKind::Qdrant => qdrant::QdrantSession::connect(config)
            .await
            .map(|s| Arc::new(s) as SharedSession),
    }
}

/// REST 型引擎（Elasticsearch / Qdrant）共用的输入解析：`METHOD path [json]`。
/// 请求体既可跟在同行，也可另起一行写多行 JSON；非 JSON 的尾随文字按注释忽略。
pub fn parse_rest_request(
    sql: &str,
) -> Result<(String, String, Option<serde_json::Value>), String> {
    let s = sql.trim();
    let (head, tail) = match s.find('\n') {
        Some(i) => (&s[..i], &s[i + 1..]),
        None => (s, ""),
    };
    let mut parts = head.trim_end().splitn(3, ' ');
    let method = parts.next().unwrap_or("GET").to_uppercase();
    let path = parts.next().unwrap_or("/").to_string();
    let raw = match parts.next() {
        Some(rest) => format!("{rest}\n{}", tail.trim()),
        None => tail.to_string(),
    };
    let raw = raw.trim().to_string();
    let body = if raw.is_empty() {
        None
    } else if raw.starts_with('{') || raw.starts_with('[') {
        Some(
            serde_json::from_str::<serde_json::Value>(&raw)
                .map_err(|e| format!("请求体 JSON 解析失败：{e}"))?,
        )
    } else {
        None
    };
    Ok((method, path, body))
}

pub fn sql_identifier(name: &str) -> String {
    // basic safety: escape quotes
    format!("`{}`", name.replace('`', "``"))
}

pub fn sql_identifier_pg(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

pub fn json_to_sql_literal(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::Null => "NULL".into(),
        serde_json::Value::Bool(b) => if *b { "1" } else { "0" }.into(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::String(s) => format!("'{}'", s.replace('\'', "''")),
        other => format!("'{}'", other.to_string().replace('\'', "''")),
    }
}

pub fn json_to_sql_literal_pg(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::Null => "NULL".into(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::String(s) => format!("'{}'", s.replace('\'', "''")),
        other => format!("'{}'", other.to_string().replace('\'', "''")),
    }
}
