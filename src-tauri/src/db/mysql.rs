use super::*;
use crate::models::ConnectionConfig;
use sqlx::mysql::MySqlPoolOptions;
use sqlx::{Column, Row, TypeInfo};
use std::collections::HashMap;
use std::sync::Mutex;

pub struct MySqlSession {
    host: String,
    port: u16,
    user: String,
    pass: String,
    default_db: String,
    connect_timeout: std::time::Duration,
    pools: Mutex<HashMap<String, sqlx::MySqlPool>>,
}

impl MySqlSession {
    pub async fn connect(config: &ConnectionConfig) -> Result<Self, String> {
        let s = Self {
            host: config.host.clone().unwrap_or_else(|| "localhost".into()),
            port: config.port.unwrap_or(3306),
            user: config.username.clone().unwrap_or_default(),
            pass: config.password.clone().unwrap_or_default(),
            default_db: config
                .database
                .clone()
                .filter(|d| !d.is_empty())
                .unwrap_or_default(),
            connect_timeout: connect_timeout(config),
            pools: Mutex::new(HashMap::new()),
        };
        // probe server (no schema)
        let pool = s.open_pool("").await?;
        sqlx::query("SELECT 1")
            .execute(&pool)
            .await
            .map_err(|e| format!("MySQL 连接失败: {e}"))?;
        Ok(s)
    }

    fn url_for(&self, database: &str) -> String {
        if database.is_empty() {
            format!(
                "mysql://{}:{}@{}:{}/",
                urlencode(&self.user),
                urlencode(&self.pass),
                self.host,
                self.port
            )
        } else {
            format!(
                "mysql://{}:{}@{}:{}/{}",
                urlencode(&self.user),
                urlencode(&self.pass),
                self.host,
                self.port,
                database
            )
        }
    }

    async fn open_pool(&self, database: &str) -> Result<sqlx::MySqlPool, String> {
        let key = database.to_string();
        if let Some(p) = self.pools.lock().unwrap().get(&key) {
            return Ok(p.clone());
        }
        let url = self.url_for(database);
        let connect = MySqlPoolOptions::new()
            .max_connections(4)
            .acquire_timeout(self.connect_timeout)
            .connect(&url);
        let pool = tokio::time::timeout(self.connect_timeout, connect)
            .await
            .map_err(|_| {
                format!(
                    "MySQL 打开库 {database:?} 超时（{}s）",
                    self.connect_timeout.as_secs()
                )
            })?
            .map_err(|e| format!("MySQL 打开库 {database:?} 失败: {e}"))?;
        self.pools.lock().unwrap().insert(key, pool.clone());
        Ok(pool)
    }

    async fn pool(&self, database: &str) -> Result<sqlx::MySqlPool, String> {
        let db = if database.is_empty() {
            self.default_db.as_str()
        } else {
            database
        };
        self.open_pool(db).await
    }
}

fn urlencode(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        match c {
            '@' | ':' | '/' | '?' | '#' | '%' | ' ' => {
                out.push_str(&format!("%{:02X}", c as u32));
            }
            _ => out.push(c),
        }
    }
    out
}

async fn exec_on(
    pool: &sqlx::MySqlPool,
    sql: &str,
) -> Result<(u64, u64), String> {
    let res = sqlx::raw_sql(sql)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    Ok((res.rows_affected(), res.last_insert_id()))
}

async fn fetch_page_on(
    pool: &sqlx::MySqlPool,
    sql: &str,
    offset: u64,
    page_size: u64,
) -> Result<(Vec<String>, Vec<Vec<serde_json::Value>>, u64), String> {
    let trimmed = sql.trim_end_matches([';', ' ', '\n', '\r', '\t']);
    let paged = format!(
        "SELECT * FROM ({trimmed}) AS __q LIMIT {page_size} OFFSET {offset}"
    );
    let rows = sqlx::query(&paged)
        .fetch_all(pool)
        .await
        .map_err(|e| e.to_string())?;
    let columns: Vec<String> = rows
        .first()
        .map(|r| r.columns().iter().map(|c| c.name().to_string()).collect())
        .unwrap_or_default();
    let data: Vec<Vec<serde_json::Value>> = rows
        .iter()
        .map(|r| {
            columns
                .iter()
                .enumerate()
                .map(|(i, _)| row_value_mysql(r, i))
                .collect()
        })
        .collect();
    let count_sql = format!("SELECT COUNT(*) FROM ({trimmed}) AS __c");
    let total: u64 = match sqlx::query(&count_sql).fetch_one(pool).await {
        Ok(r) => r.try_get::<i64, _>(0).unwrap_or(0).max(0) as u64,
        Err(_) => offset + data.len() as u64,
    };
    Ok((columns, data, total))
}

fn row_value_mysql(row: &sqlx::mysql::MySqlRow, idx: usize) -> serde_json::Value {
    let col = &row.columns()[idx];
    let type_name = col.type_info().name();

    if type_name.contains("INT") {
        if let Ok(v) = row.try_get::<Option<i64>, _>(idx) {
            return match v {
                Some(n) => serde_json::Value::from(n),
                None => serde_json::Value::Null,
            };
        }
        if let Ok(v) = row.try_get::<Option<i32>, _>(idx) {
            return match v {
                Some(n) => serde_json::Value::from(n),
                None => serde_json::Value::Null,
            };
        }
    }
    if type_name.contains("DECIMAL") {
        // sqlx 把 DECIMAL 解成 BigDecimal，按 f64 取会失败并静默变 NULL
        if let Ok(v) = row.try_get::<Option<bigdecimal::BigDecimal>, _>(idx) {
            return match v {
                Some(d) => serde_json::Value::String(d.to_string()),
                None => serde_json::Value::Null,
            };
        }
    }
    if type_name.contains("FLOAT")
        || type_name.contains("DOUBLE")
        || type_name.contains("DECIMAL")
    {
        if let Ok(v) = row.try_get::<Option<f64>, _>(idx) {
            return match v {
                Some(n) => serde_json::Number::from_f64(n)
                    .map(serde_json::Value::Number)
                    .unwrap_or(serde_json::Value::Null),
                None => serde_json::Value::Null,
            };
        }
    }
    if type_name.contains("DATETIME") || type_name.contains("TIMESTAMP") {
        if let Ok(v) = row.try_get::<Option<chrono::NaiveDateTime>, _>(idx) {
            return match v {
                Some(t) => {
                    serde_json::Value::String(t.format("%Y-%m-%d %H:%M:%S%.f").to_string())
                }
                None => serde_json::Value::Null,
            };
        }
    }
    if type_name.contains("DATE") {
        if let Ok(v) = row.try_get::<Option<chrono::NaiveDate>, _>(idx) {
            return match v {
                Some(t) => serde_json::Value::String(t.to_string()),
                None => serde_json::Value::Null,
            };
        }
    }
    if type_name.contains("BOOL") || type_name == "TINYINT" {
        if let Ok(v) = row.try_get::<Option<bool>, _>(idx) {
            return match v {
                Some(b) => serde_json::Value::Bool(b),
                None => serde_json::Value::Null,
            };
        }
    }
    if let Ok(v) = row.try_get::<Option<String>, _>(idx) {
        return match v {
            Some(s) => serde_json::Value::String(s),
            None => serde_json::Value::Null,
        };
    }
    if let Ok(v) = row.try_get::<Option<Vec<u8>>, _>(idx) {
        return match v {
            // 元数据列常带 *_bin 排序规则，sqlx 按字节返回；可打印文本直接还原
            Some(b) => match std::str::from_utf8(&b) {
                Ok(text) if printable(text) => serde_json::Value::String(text.to_string()),
                _ => serde_json::Value::String(format!("<blob {} bytes>", b.len())),
            },
            None => serde_json::Value::Null,
        };
    }
    serde_json::Value::Null
}

#[async_trait]
impl DbSession for MySqlSession {
    fn kind(&self) -> DbKind {
        DbKind::Mysql
    }

    fn plan_create_table(&self, spec: &TableSpec) -> Result<Vec<String>, String> {
        plan_create_table_mysql(spec)
    }

    async fn ping(&self) -> Result<(), String> {
        let pool = self.pool("").await?;
        sqlx::query("SELECT 1")
            .execute(&pool)
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    async fn list_databases(&self) -> Result<Vec<String>, String> {
        let pool = self.open_pool("").await?;
        let rows = sqlx::query(
            "SELECT schema_name FROM information_schema.schemata \
             WHERE schema_name NOT IN ('information_schema','performance_schema','mysql','sys') \
             ORDER BY schema_name",
        )
        .fetch_all(&pool)
        .await
        .map_err(|e| e.to_string())?;
        Ok(rows
            .iter()
            .map(|r| text_at(r, 0))
            .collect())
    }

    async fn list_tables(&self, database: &str, _schema: Option<&str>) -> Result<Vec<TableInfo>, String> {
        let pool = self.open_pool("").await?;
        let rows = sqlx::query(
            "SELECT table_name, table_type, table_comment, table_rows \
             FROM information_schema.tables WHERE table_schema = ? ORDER BY table_name",
        )
        .bind(database)
        .fetch_all(&pool)
        .await
        .map_err(|e| e.to_string())?;
        Ok(rows
            .iter()
            .map(|r| TableInfo {
                name: text_at(r, 0),
                schema: Some(database.to_string()),
                table_type: text_opt(r, 1),
                comment: text_opt(r, 2).filter(|v| !v.is_empty()),
                row_estimate: r.try_get(3).ok(),
                tablespace: None,
            })
            .collect())
    }

    async fn list_columns(&self, database: &str, _schema: Option<&str>, table: &str) -> Result<Vec<ColumnInfo>, String> {
        let pool = self.open_pool("").await?;
        let rows = sqlx::query(
            "SELECT c.column_name, c.column_type, c.is_nullable, c.column_default, c.extra, \
                    c.character_maximum_length, c.column_key, c.column_comment \
             FROM information_schema.columns c \
             WHERE c.table_schema = ? AND c.table_name = ? \
             ORDER BY c.ordinal_position",
        )
        .bind(database)
        .bind(table)
        .fetch_all(&pool)
        .await
        .map_err(|e| e.to_string())?;
        Ok(rows
            .iter()
            .map(|r| {
                let key = text_at(r, 6);
                let default_raw = text_opt(r, 3).filter(|v| !v.is_empty());
                ColumnInfo {
                    name: text_at(r, 0),
                    data_type: text_at(r, 1),
                    nullable: text_at(r, 2).eq_ignore_ascii_case("YES"),
                    primary_key: key == "PRI",
                    default: default_raw,
                    comment: text_opt(r, 7).filter(|v| !v.is_empty()),
                    extra: text_opt(r, 4).filter(|v| !v.is_empty()),
                    max_length: r.try_get(5).ok(),
                }
            })
            .collect())
    }

    async fn list_indexes(&self, database: &str, _schema: Option<&str>, table: &str) -> Result<Vec<IndexInfo>, String> {
        let pool = self.open_pool("").await?;
        let rows = sqlx::query(
            "SELECT index_name, column_name, non_unique, seq_in_index \
             FROM information_schema.statistics \
             WHERE table_schema = ? AND table_name = ? \
             ORDER BY index_name, seq_in_index",
        )
        .bind(database)
        .bind(table)
        .fetch_all(&pool)
        .await
        .map_err(|e| e.to_string())?;
        let mut map: std::collections::BTreeMap<String, IndexInfo> = Default::default();
        for r in &rows {
            let name = text_at(r, 0);
            let col = text_at(r, 1);
            let non_unique: i64 = r.try_get(2).unwrap_or(1);
            let e = map.entry(name.clone()).or_insert_with(|| IndexInfo {
                name: name.clone(),
                columns: vec![],
                unique: non_unique == 0,
                primary: name == "PRIMARY",
            });
            e.columns.push(col);
        }
        Ok(map.into_values().collect())
    }

    async fn list_foreign_keys(
        &self,
        database: &str,
        _schema: Option<&str>,
        table: &str,
    ) -> Result<Vec<ForeignKeyInfo>, String> {
        let pool = self.open_pool("").await?;
        let rows = sqlx::query(
            "SELECT constraint_name, column_name, referenced_table_name, referenced_column_name \
             FROM information_schema.key_column_usage \
             WHERE table_schema = ? AND table_name = ? AND referenced_table_name IS NOT NULL",
        )
        .bind(database)
        .bind(table)
        .fetch_all(&pool)
        .await
        .map_err(|e| e.to_string())?;
        Ok(rows
            .iter()
            .map(|r| ForeignKeyInfo {
                name: text_at(r, 0),
                column: text_at(r, 1),
                ref_table: text_at(r, 2),
                ref_column: text_at(r, 3),
            })
            .collect())
    }

    async fn query_page(
        &self,
        database: &str,
        sql: &str,
        page: u64,
        page_size: u64,
    ) -> Result<QueryPage, String> {
        let start = std::time::Instant::now();
        let pool = self.pool(database).await?;
        let offset = page.saturating_sub(1) * page_size;
        let (columns, data, total) = fetch_page_on(&pool, sql, offset, page_size).await?;
        Ok(QueryPage {
            columns,
            rows: data,
            total,
            page,
            page_size,
            elapsed_ms: start.elapsed().as_millis() as u64,
            message: Some(if database.is_empty() {
                "server".into()
            } else {
                format!("db={database}")
            }),
        })
    }

    async fn execute(&self, database: &str, sql: &str) -> Result<ExecResult, String> {
        let upper = sql.trim_start().to_ascii_uppercase();
        let is_server_cmd = upper.starts_with("CREATE DATABASE")
            || upper.starts_with("DROP DATABASE")
            || upper.starts_with("CREATE SCHEMA");
        let pool = if is_server_cmd {
            self.open_pool("").await?
        } else {
            self.pool(database).await?
        };
        let (affected, last_id) = exec_on(&pool, sql).await?;
        Ok(ExecResult {
            affected_rows: affected,
            last_insert_id: if last_id == 0 {
                None
            } else {
                Some(last_id as i64)
            },
            message: Some(format!("影响 {} 行", affected)),
        })
    }

    /// 被外键引用的表 MySQL 不让 TRUNCATE，退化成 DELETE —— 语义同样是清空全部行
    async fn truncate_table(&self, database: &str, table: &str) -> Result<ExecResult, String> {
        let pool = self.pool(database).await?;
        let q = format!("`{}`", table.replace('`', "``"));
        let (affected, how) = match sqlx::raw_sql(&format!("TRUNCATE TABLE {q}"))
            .execute(&pool)
            .await
        {
            Ok(r) => (r.rows_affected(), "TRUNCATE"),
            Err(e1) => {
                let r = sqlx::raw_sql(&format!("DELETE FROM {q}"))
                    .execute(&pool)
                    .await
                    .map_err(|e2| format!("{e1}；改用 DELETE 也失败：{e2}"))?;
                (r.rows_affected(), "DELETE")
            }
        };
        Ok(ExecResult {
            affected_rows: affected,
            last_insert_id: None,
            message: Some(format!("已清空 {table}（{how}，影响 {affected} 行）")),
        })
    }

    async fn drop_table(&self, database: &str, table: &str) -> Result<ExecResult, String> {
        let pool = self.pool(database).await?;
        let q = format!("`{}`", table.replace('`', "``"));
        sqlx::raw_sql(&format!("DROP TABLE IF EXISTS {q}"))
            .execute(&pool)
            .await
            .map_err(|e| e.to_string())?;
        Ok(ExecResult {
            affected_rows: 0,
            last_insert_id: None,
            message: Some(format!("已删除表 {table}")),
        })
    }

    async fn insert_row(
        &self,
        database: &str,
        table: &str,
        data: serde_json::Map<String, serde_json::Value>,
    ) -> Result<ExecResult, String> {
        if data.is_empty() {
            return Err("数据为空".into());
        }
        let cols: Vec<String> = data.keys().map(|k| sql_identifier(k)).collect();
        let vals: Vec<String> = data.values().map(json_to_sql_literal).collect();
        let sql = format!(
            "INSERT INTO {} ({}) VALUES ({})",
            sql_identifier(table),
            cols.join(", "),
            vals.join(", ")
        );
        self.execute(database, &sql).await
    }

    async fn update_row(
        &self,
        database: &str,
        table: &str,
        pk: serde_json::Map<String, serde_json::Value>,
        data: serde_json::Map<String, serde_json::Value>,
    ) -> Result<ExecResult, String> {
        if pk.is_empty() || data.is_empty() {
            return Err("主键与更新数据不能为空".into());
        }
        let sets: Vec<String> = data
            .iter()
            .map(|(k, v)| format!("{} = {}", sql_identifier(k), json_to_sql_literal(v)))
            .collect();
        let wheres: Vec<String> = pk
            .iter()
            .map(|(k, v)| format!("{} = {}", sql_identifier(k), json_to_sql_literal(v)))
            .collect();
        let sql = format!(
            "UPDATE {} SET {} WHERE {}",
            sql_identifier(table),
            sets.join(", "),
            wheres.join(" AND ")
        );
        self.execute(database, &sql).await
    }

    async fn delete_row(
        &self,
        database: &str,
        table: &str,
        pk: serde_json::Map<String, serde_json::Value>,
    ) -> Result<ExecResult, String> {
        if pk.is_empty() {
            return Err("主键不能为空".into());
        }
        let wheres: Vec<String> = pk
            .iter()
            .map(|(k, v)| format!("{} = {}", sql_identifier(k), json_to_sql_literal(v)))
            .collect();
        let sql = format!(
            "DELETE FROM {} WHERE {}",
            sql_identifier(table),
            wheres.join(" AND ")
        );
        self.execute(database, &sql).await
    }

    fn db_caps(&self) -> DbCaps {
        DbCaps {
            charset: true,
            // MySQL 的库级选项只有字符集与排序规则
            ..Default::default()
        }
    }

    async fn db_options(&self, database: &str) -> Result<DbOptions, String> {
        let pool = self.open_pool("").await?;
        let charsets = sqlx::query("SHOW CHARACTER SET")
            .fetch_all(&pool)
            .await
            .unwrap_or_default()
            .iter()
            .filter_map(|r| text_opt(r, 0))
            .collect();
        let collation_names = sqlx::query("SHOW COLLATION")
            .fetch_all(&pool)
            .await
            .unwrap_or_default()
            .iter()
            .filter_map(|r| text_opt(r, 0))
            .collect();
        let mut current = None;
        if !database.is_empty() {
            if let Some(row) = sqlx::query(
                "SELECT DEFAULT_CHARACTER_SET_NAME, DEFAULT_COLLATION_NAME \
                 FROM information_schema.schemata WHERE schema_name = ?",
            )
            .bind(database)
            .fetch_optional(&pool)
            .await
            .map_err(|e| e.to_string())?
            {
                current = Some(DbSpec {
                    name: database.to_string(),
                    charset: text_opt(&row, 0),
                    collation: text_opt(&row, 1),
                    ..Default::default()
                });
            }
        }
        Ok(DbOptions {
            caps: self.db_caps(),
            charsets,
            collation_names,
            current,
            ..Default::default()
        })
    }

    fn plan_database(&self, action: &str, spec: &DbSpec) -> Result<Vec<String>, String> {
        let name = spec.name.trim();
        if name.is_empty() {
            return Err("数据库名称不能为空".into());
        }
        let id = sql_identifier(name);
        let mut tail = String::new();
        if let Some(cs) = spec.charset.as_deref().map(str::trim) {
            if !cs.is_empty() {
                tail.push_str(&format!(" DEFAULT CHARACTER SET {}", sql_identifier(cs)));
            }
        }
        if let Some(co) = spec.collation.as_deref().map(str::trim) {
            if !co.is_empty() {
                tail.push_str(&format!(" COLLATE {}", sql_identifier(co)));
            }
        }
        let out = match action {
            "create" => vec![format!("CREATE DATABASE IF NOT EXISTS {id}{tail}")],
            "alter" => {
                if tail.trim().is_empty() {
                    return Err("没有需要修改的字符集或排序规则".into());
                }
                vec![format!("ALTER DATABASE {id}{tail}")]
            }
            "drop" => vec![format!("DROP DATABASE {id}")],
            other => return Err(format!("未知操作: {other}")),
        };
        Ok(out)
    }
}

/// 可视化建表：MySQL 方言
pub fn plan_create_table_mysql(spec: &TableSpec) -> Result<Vec<String>, String> {
        let (cols, pks) = crate::db::validate_table_spec(spec)?;
        let mut parts: Vec<String> = Vec::new();
        for c in &cols {
            let raw_ty = c.data_type.as_deref().unwrap_or("").trim();
            let base = raw_ty.split('(').next().unwrap_or("").trim();
            if mysql_needs_length(base) && !raw_ty.contains('(') {
                return Err(format!(
                    "字段 {} 的类型 {base} 需要长度，例如 {}(64)",
                    c.name.trim(),
                    base
                ));
            }
            let mut p = format!(
                "  {} {}",
                sql_identifier(c.name.trim()),
                raw_ty
            );
            if c.nullable == Some(false) {
                p.push_str(" NOT NULL");
            }
            if c.auto_increment == Some(true) {
                p.push_str(" AUTO_INCREMENT");
            }
            let d = c.default_value.as_deref().unwrap_or("").trim();
            if !d.is_empty() {
                p.push_str(&format!(" DEFAULT {}", mysql_default_expr(d)));
            }
            let cm = c.comment.as_deref().unwrap_or("").trim();
            if !cm.is_empty() {
                p.push_str(&format!(" COMMENT '{}'", cm.replace('\'', "''")));
            }
            parts.push(p);
        }
        if !pks.is_empty() {
            let list: Vec<String> = pks.iter().map(|p| sql_identifier(p)).collect();
            parts.push(format!("  PRIMARY KEY ({})", list.join(", ")));
        }
        let mut tail = String::new();
        let engine = spec.engine.as_deref().unwrap_or("").trim();
        if !engine.is_empty() {
            tail.push_str(&format!(" ENGINE={}", engine.replace('`', "")));
        }
        let cs = spec.charset.as_deref().unwrap_or("").trim();
        if !cs.is_empty() {
            tail.push_str(&format!(" DEFAULT CHARSET={}", cs.replace('`', "")));
        }
        let tc = spec.comment.as_deref().unwrap_or("").trim();
        if !tc.is_empty() {
            tail.push_str(&format!(" COMMENT='{}'", tc.replace('\'', "''")));
        }
        Ok(vec![format!(
            "CREATE TABLE {} (\n{}\n){}",
            sql_identifier(spec.name.trim()),
            parts.join(",\n"),
            tail
        )])
}

/// 这些 MySQL 类型必须带长度，否则服务端报 1064
fn mysql_needs_length(base: &str) -> bool {
    matches!(
        base.to_ascii_uppercase().as_str(),
        "VARCHAR" | "CHAR" | "BIT" | "BINARY" | "VARBINARY" | "ENUM" | "SET"
    )
}

fn mysql_default_expr(d: &str) -> String {
    let up = d.to_ascii_uppercase();
    if d.eq_ignore_ascii_case("null")
        || up.starts_with("CURRENT_TIMESTAMP")
        || up.starts_with("NOW(")
        || up.starts_with("UUID(")
        || up.starts_with("(")
        || d.parse::<f64>().is_ok()
    {
        d.to_string()
    } else {
        format!("'{}'", d.replace('\'', "''"))
    }
}

/// information_schema 的 COLUMN_TYPE / COLUMN_KEY 等带 *_bin 排序规则，
/// 直接 try_get::<String> 会失败并读成空值，统一走这里。
fn text_at(row: &sqlx::mysql::MySqlRow, idx: usize) -> String {
    text_opt(row, idx).unwrap_or_default()
}

fn text_opt(row: &sqlx::mysql::MySqlRow, idx: usize) -> Option<String> {
    if let Ok(Some(v)) = row.try_get::<Option<String>, _>(idx) {
        return Some(v);
    }
    if let Ok(Some(v)) = row.try_get::<Option<Vec<u8>>, _>(idx) {
        return Some(String::from_utf8_lossy(&v).into_owned());
    }
    None
}

/// 只有全是可打印字符时才当作文本展示
fn printable(text: &str) -> bool {
    // 制表 / 换行属于控制符但可安全展示
    text.chars().all(|c| !c.is_control() || c.is_whitespace())
}
