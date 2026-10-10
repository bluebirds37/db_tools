use super::*;
use crate::models::ConnectionConfig;
use sqlx::postgres::PgPoolOptions;
use sqlx::{Column, Row};
use std::collections::HashMap;
use std::sync::Mutex;

pub struct PostgresSession {
    host: String,
    port: u16,
    user: String,
    pass: String,
    default_db: String,
    connect_timeout: std::time::Duration,
    pools: Mutex<HashMap<String, sqlx::PgPool>>,
}

impl PostgresSession {
    pub async fn connect(config: &ConnectionConfig) -> Result<Self, String> {
        let s = Self {
            host: config.host.clone().unwrap_or_else(|| "localhost".into()),
            port: config.port.unwrap_or(5432),
            user: config.username.clone().unwrap_or_else(|| "postgres".into()),
            pass: config.password.clone().unwrap_or_default(),
            default_db: config
                .database
                .clone()
                .filter(|d| !d.is_empty())
                .unwrap_or_else(|| "postgres".into()),
            connect_timeout: connect_timeout(config),
            pools: Mutex::new(HashMap::new()),
        };
        let pool = s.open_pool(&s.default_db.clone()).await?;
        sqlx::query("SELECT 1")
            .execute(&pool)
            .await
            .map_err(|e| format!("PostgreSQL 连接失败: {e}"))?;
        Ok(s)
    }

    fn url_for(&self, database: &str) -> String {
        format!(
            "postgres://{}:{}@{}:{}/{}",
            self.user, self.pass, self.host, self.port, database
        )
    }

    async fn open_pool(&self, database: &str) -> Result<sqlx::PgPool, String> {
        let db = if database.is_empty() {
            self.default_db.as_str()
        } else {
            database
        };
        if let Some(p) = self.pools.lock().unwrap().get(db) {
            return Ok(p.clone());
        }
        let url = self.url_for(db);
        let connect = PgPoolOptions::new()
            .max_connections(3)
            .acquire_timeout(self.connect_timeout)
            .connect(&url);
        let pool = tokio::time::timeout(self.connect_timeout, connect)
            .await
            .map_err(|_| {
                format!(
                    "PostgreSQL 打开库 {db} 失败：超过 {}s 无响应",
                    self.connect_timeout.as_secs()
                )
            })?
            .map_err(|e| format!("PostgreSQL 打开库 {db} 失败: {e}"))?;
        self.pools
            .lock()
            .unwrap()
            .insert(db.to_string(), pool.clone());
        Ok(pool)
    }
}

fn row_value_pg(row: &sqlx::postgres::PgRow, idx: usize) -> serde_json::Value {
    use sqlx::TypeInfo;
    let type_name = row.columns()[idx].type_info().name();

    if type_name == "BOOL" {
        return match row.try_get::<Option<bool>, _>(idx) {
            Ok(Some(b)) => serde_json::Value::Bool(b),
            _ => serde_json::Value::Null,
        };
    }
    if type_name.starts_with("INT")
        || type_name == "BIGINT"
        || type_name == "SMALLINT"
        || type_name == "SERIAL"
        || type_name == "BIGSERIAL"
    {
        return match row.try_get::<Option<i64>, _>(idx) {
            Ok(Some(n)) => serde_json::Value::from(n),
            _ => match row.try_get::<Option<i32>, _>(idx) {
                Ok(Some(n)) => serde_json::Value::from(n),
                _ => serde_json::Value::Null,
            },
        };
    }
    if type_name == "NUMERIC" {
        // 同上：NUMERIC 是 BigDecimal，直接落 f64 会读成 NULL
        if let Ok(v) = row.try_get::<Option<bigdecimal::BigDecimal>, _>(idx) {
            return match v {
                Some(d) => serde_json::Value::String(d.to_string()),
                _ => serde_json::Value::Null,
            };
        }
    }
    if type_name.contains("FLOAT")
        || type_name.contains("DOUBLE")
        || type_name == "NUMERIC"
        || type_name == "REAL"
    {
        return match row.try_get::<Option<f64>, _>(idx) {
            Ok(Some(n)) => serde_json::Number::from_f64(n)
                .map(serde_json::Value::Number)
                .unwrap_or(serde_json::Value::Null),
            _ => serde_json::Value::Null,
        };
    }
    if type_name.contains("TIMESTAMP") {
        return match row.try_get::<Option<chrono::DateTime<chrono::Utc>>, _>(idx) {
            Ok(Some(t)) => serde_json::Value::String(t.to_rfc3339()),
            _ => match row.try_get::<Option<chrono::NaiveDateTime>, _>(idx) {
                Ok(Some(t)) => serde_json::Value::String(t.to_string()),
                _ => serde_json::Value::Null,
            },
        };
    }
    if type_name == "DATE" {
        return match row.try_get::<Option<chrono::NaiveDate>, _>(idx) {
            Ok(Some(d)) => serde_json::Value::String(d.to_string()),
            _ => serde_json::Value::Null,
        };
    }
    if type_name == "JSON" || type_name == "JSONB" {
        return match row.try_get::<Option<serde_json::Value>, _>(idx) {
            Ok(Some(v)) => v,
            _ => serde_json::Value::Null,
        };
    }
    match row.try_get::<Option<String>, _>(idx) {
        Ok(Some(s)) => serde_json::Value::String(s),
        _ => serde_json::Value::Null,
    }
}

#[async_trait]
impl DbSession for PostgresSession {
    fn kind(&self) -> DbKind {
        DbKind::Postgres
    }

    fn plan_create_table(&self, spec: &TableSpec) -> Result<Vec<String>, String> {
        plan_create_table_pg(spec)
    }

    async fn ping(&self) -> Result<(), String> {
        let pool = self.open_pool("").await?;
        sqlx::query("SELECT 1")
            .execute(&pool)
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    async fn list_databases(&self) -> Result<Vec<String>, String> {
        let pool = self.open_pool("").await?;
        let rows = sqlx::query(
            "SELECT datname FROM pg_database WHERE datistemplate = false ORDER BY datname",
        )
        .fetch_all(&pool)
        .await
        .map_err(|e| e.to_string())?;
        Ok(rows
            .iter()
            .map(|r| r.try_get::<String, _>(0).unwrap_or_default())
            .collect())
    }

    async fn list_schemas(&self, database: &str) -> Result<Vec<String>, String> {
        let pool = self.open_pool(database).await?;
        let rows = sqlx::query(
            "SELECT nspname FROM pg_namespace \
             WHERE nspname NOT LIKE 'pg\\_%' AND nspname <> 'information_schema' \
             ORDER BY nspname",
        )
        .fetch_all(&pool)
        .await
        .map_err(|e| e.to_string())?;
        Ok(rows
            .iter()
            .map(|r| r.try_get::<String, _>(0).unwrap_or_default())
            .collect())
    }

    async fn list_tables(
        &self,
        database: &str,
        schema: Option<&str>,
    ) -> Result<Vec<TableInfo>, String> {
        let pool = self.open_pool(database).await?;
        let rows = if let Some(sch) = schema.filter(|s| !s.is_empty()) {
            sqlx::query(
                "SELECT c.relname, n.nspname, c.relkind, obj_description(c.oid) as comment, \
                        GREATEST(c.reltuples::bigint, 0) as est, \
                        COALESCE(ts.spcname, 'pg_default') as tablespace \
                 FROM pg_class c \
                 JOIN pg_namespace n ON n.oid = c.relnamespace \
                 LEFT JOIN pg_tablespace ts ON ts.oid = c.reltablespace \
                 WHERE n.nspname = $1 AND c.relkind IN ('r','p','v','m') \
                 ORDER BY c.relname",
            )
            .bind(sch)
            .fetch_all(&pool)
            .await
            .map_err(|e| e.to_string())?
        } else {
            sqlx::query(
                "SELECT c.relname, n.nspname, c.relkind, obj_description(c.oid) as comment, \
                        GREATEST(c.reltuples::bigint, 0) as est, \
                        COALESCE(ts.spcname, 'pg_default') as tablespace \
                 FROM pg_class c \
                 JOIN pg_namespace n ON n.oid = c.relnamespace \
                 LEFT JOIN pg_tablespace ts ON ts.oid = c.reltablespace \
                 WHERE n.nspname NOT IN ('pg_catalog','information_schema') \
                   AND n.nspname NOT LIKE 'pg\\_%' \
                   AND c.relkind IN ('r','p','v','m') \
                 ORDER BY n.nspname, c.relname",
            )
            .fetch_all(&pool)
            .await
            .map_err(|e| e.to_string())?
        };
        Ok(rows
            .iter()
            .map(|r| {
                let kind: String = r.try_get(2).unwrap_or_default();
                TableInfo {
                    name: r.try_get(0).unwrap_or_default(),
                    schema: Some(r.try_get(1).unwrap_or_else(|_| "public".into())),
                    table_type: Some(match kind.as_str() {
                        "v" => "VIEW".into(),
                        "m" => "MATERIALIZED VIEW".into(),
                        _ => "BASE TABLE".into(),
                    }),
                    comment: r.try_get(3).ok().filter(|s: &String| !s.is_empty()),
                    row_estimate: r.try_get(4).ok(),
                    tablespace: r
                        .try_get::<String, _>(5)
                        .ok()
                        .filter(|s: &String| !s.is_empty()),
                }
            })
            .collect())
    }

    async fn list_tablespaces(&self) -> Result<Vec<TablespaceInfo>, String> {
        let pool = self.open_pool("").await?;
        let full = sqlx::query(
            "SELECT spcname, pg_get_userbyid(spcowner) AS owner, \
                    COALESCE(pg_tablespace_location(oid), '') AS location, \
                    pg_size_pretty(pg_tablespace_size(oid)) AS size, \
                    COALESCE(obj_description(oid, 'pg_tablespace'), '') AS comment \
             FROM pg_tablespace ORDER BY spcname",
        )
        .fetch_all(&pool)
        .await;
        let rows = match full {
            Ok(r) => r,
            Err(e) => {
                crate::logger::warn("list_tablespaces", format!("fallback: {e}"));
                sqlx::query(
                    "SELECT spcname, pg_get_userbyid(spcowner) AS owner, \
                            COALESCE(pg_tablespace_location(oid), '') AS location, \
                            NULL::text AS size, \
                            COALESCE(obj_description(oid, 'pg_tablespace'), '') AS comment \
                     FROM pg_tablespace ORDER BY spcname",
                )
                .fetch_all(&pool)
                .await
                .map_err(|e2| format!("查询表空间失败: {e2}"))?
            }
        };
        Ok(rows
            .iter()
            .map(|r| TablespaceInfo {
                name: r.try_get(0).unwrap_or_default(),
                owner: r.try_get(1).ok().filter(|s: &String| !s.is_empty()),
                location: r.try_get(2).ok().filter(|s: &String| !s.is_empty()),
                size: r.try_get(3).ok(),
                comment: r.try_get(4).ok().filter(|s: &String| !s.is_empty()),
            })
            .collect())
    }

    async fn describe_table_extra(
        &self,
        database: &str,
        schema: Option<&str>,
        table: &str,
    ) -> Result<TableExtra, String> {
        let pool = self.open_pool(database).await?;
        let sch = schema.filter(|s| !s.is_empty()).unwrap_or("public");
        let rows = sqlx::query(
            "SELECT n.nspname, COALESCE(ts.spcname, 'pg_default') AS tablespace, \
                    pg_size_pretty(pg_total_relation_size(c.oid)) AS size \
             FROM pg_class c \
             JOIN pg_namespace n ON n.oid = c.relnamespace \
             LEFT JOIN pg_tablespace ts ON ts.oid = c.reltablespace \
             WHERE c.relname = $1 AND n.nspname = $2 \
               AND c.relkind IN ('r','p','v','m') LIMIT 1",
        )
        .bind(table)
        .bind(sch)
        .fetch_all(&pool)
        .await
        .map_err(|e| e.to_string())?;
        let r = rows.first();
        Ok(TableExtra {
            schema: r.and_then(|r| r.try_get::<String, _>(0).ok()),
            tablespace: r.and_then(|r| r.try_get::<String, _>(1).ok()),
            size: r.and_then(|r| r.try_get::<String, _>(2).ok()),
        })
    }

    async fn list_columns(
        &self,
        database: &str,
        schema: Option<&str>,
        table: &str,
    ) -> Result<Vec<ColumnInfo>, String> {
        let pool = self.open_pool(database).await?;
        let sch = schema.filter(|s| !s.is_empty()).unwrap_or("public");
        let rows = sqlx::query(
            "SELECT column_name, \
                    (SELECT format_type(a.atttypid, a.atttypmod) \
                     FROM pg_class c \
                     JOIN pg_namespace n ON n.oid = c.relnamespace \
                     JOIN pg_attribute a ON a.attrelid = c.oid \
                     WHERE c.relname = information_schema.columns.table_name \
                       AND n.nspname = information_schema.columns.table_schema \
                       AND a.attnum = information_schema.columns.ordinal_position \
                       AND a.attisdropped = false) as full_type, \
                    is_nullable, column_default, \
                    (SELECT d.description \
                     FROM pg_class c \
                     JOIN pg_namespace n ON n.oid = c.relnamespace \
                     JOIN pg_attribute a ON a.attrelid = c.oid \
                     LEFT JOIN pg_description d ON d.objoid = c.oid AND d.objsubid = a.attnum \
                     WHERE c.relname = information_schema.columns.table_name \
                       AND n.nspname = information_schema.columns.table_schema \
                       AND a.attnum = information_schema.columns.ordinal_position \
                       AND a.attisdropped = false) as comment, \
                    character_maximum_length \
             FROM information_schema.columns \
             WHERE table_schema=$1 AND table_name=$2 \
             ORDER BY ordinal_position",
        )
        .bind(sch)
        .bind(table)
        .fetch_all(&pool)
        .await
        .map_err(|e| e.to_string())?;
        let pk_rows = sqlx::query(
            "SELECT a.attname FROM pg_index i \
             JOIN pg_attribute a ON a.attrelid = i.indrelid AND a.attnum = ANY(i.indkey) \
             JOIN pg_class c ON c.oid = i.indrelid \
             JOIN pg_namespace n ON n.oid = c.relnamespace \
             WHERE i.indisprimary AND c.relname = $1 AND n.nspname = $2",
        )
        .bind(table)
        .bind(sch)
        .fetch_all(&pool)
        .await
        .unwrap_or_default();
        let pks: std::collections::HashSet<String> = pk_rows
            .iter()
            .map(|r| r.try_get::<String, _>(0).unwrap_or_default())
            .collect();
        Ok(rows
            .iter()
            .map(|r| {
                let name: String = r.try_get(0).unwrap_or_default();
                ColumnInfo {
                    name: name.clone(),
                    data_type: r.try_get(1).unwrap_or_default(),
                    nullable: r
                        .try_get::<String, _>(2)
                        .map(|s| s.eq_ignore_ascii_case("YES"))
                        .unwrap_or(true),
                    primary_key: pks.contains(&name),
                    default: r.try_get(3).ok().filter(|s: &String| !s.is_empty()),
                    comment: r.try_get(4).ok().filter(|s: &String| !s.is_empty()),
                    extra: None,
                    max_length: r.try_get(5).ok(),
                }
            })
            .collect())
    }

    async fn list_indexes(
        &self,
        database: &str,
        schema: Option<&str>,
        table: &str,
    ) -> Result<Vec<IndexInfo>, String> {
        let pool = self.open_pool(database).await?;
        let sch = schema.filter(|s| !s.is_empty()).unwrap_or("public");
        let rows = sqlx::query(
            "SELECT i.relname as index_name, ix.indisunique, ix.indisprimary, a.attname \
             FROM pg_class t \
             JOIN pg_namespace n ON n.oid = t.relnamespace \
             JOIN pg_index ix ON t.oid = ix.indrelid \
             JOIN pg_class i ON i.oid = ix.indexrelid \
             JOIN pg_attribute a ON a.attrelid = t.oid AND a.attnum = ANY(ix.indkey) \
             WHERE t.relname = $1 AND n.nspname = $2 \
             ORDER BY i.relname, a.attname",
        )
        .bind(table)
        .bind(sch)
        .fetch_all(&pool)
        .await
        .map_err(|e| e.to_string())?;
        let mut map: std::collections::BTreeMap<String, IndexInfo> = Default::default();
        for r in &rows {
            let name: String = r.try_get(0).unwrap_or_default();
            let unique: bool = r.try_get(1).unwrap_or(false);
            let primary: bool = r.try_get(2).unwrap_or(false);
            let col: String = r.try_get(3).unwrap_or_default();
            let e = map.entry(name.clone()).or_insert_with(|| IndexInfo {
                name: name.clone(),
                columns: vec![],
                unique,
                primary,
            });
            e.columns.push(col);
        }
        Ok(map.into_values().collect())
    }

    async fn list_foreign_keys(
        &self,
        database: &str,
        schema: Option<&str>,
        table: &str,
    ) -> Result<Vec<ForeignKeyInfo>, String> {
        let pool = self.open_pool(database).await?;
        let sch = schema.filter(|s| !s.is_empty()).unwrap_or("public");
        let rows = sqlx::query(
            "SELECT tc.constraint_name, kcu.column_name, ccu.table_name, ccu.column_name \
             FROM information_schema.table_constraints tc \
             JOIN information_schema.key_column_usage kcu ON tc.constraint_name = kcu.constraint_name \
             JOIN information_schema.constraint_column_usage ccu ON ccu.constraint_name = tc.constraint_name \
             WHERE tc.constraint_type = 'FOREIGN KEY' AND tc.table_name = $1 AND tc.table_schema = $2",
        )
        .bind(table)
        .bind(sch)
        .fetch_all(&pool)
        .await
        .map_err(|e| e.to_string())?;
        Ok(rows
            .iter()
            .map(|r| ForeignKeyInfo {
                name: r.try_get(0).unwrap_or_default(),
                column: r.try_get(1).unwrap_or_default(),
                ref_table: r.try_get(2).unwrap_or_default(),
                ref_column: r.try_get(3).unwrap_or_default(),
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
        let pool = self.open_pool(database).await?;
        let trimmed = sql.trim_end_matches([';', ' ', '\n', '\r', '\t']);
        let offset = page.saturating_sub(1) * page_size;
        let paged = format!(
            "SELECT * FROM ({}) AS __q LIMIT {} OFFSET {}",
            trimmed, page_size, offset
        );
        let rows = sqlx::query(&paged)
            .fetch_all(&pool)
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
                    .map(|(i, _)| row_value_pg(r, i))
                    .collect()
            })
            .collect();
        let count_sql = format!("SELECT COUNT(*) FROM ({}) AS __c", trimmed);
        let total: u64 = match sqlx::query(&count_sql).fetch_one(&pool).await {
            Ok(r) => r.try_get::<i64, _>(0).unwrap_or(0).max(0) as u64,
            Err(_) => offset + data.len() as u64,
        };
        Ok(QueryPage {
            columns,
            rows: data,
            total,
            page,
            page_size,
            elapsed_ms: start.elapsed().as_millis() as u64,
            message: Some(format!("db={database}")),
        })
    }

    async fn execute(&self, database: &str, sql: &str) -> Result<ExecResult, String> {
        let pool = self.open_pool(database).await?;
        let res = sqlx::raw_sql(sql)
            .execute(&pool)
            .await
            .map_err(|e| e.to_string())?;
        Ok(ExecResult {
            affected_rows: res.rows_affected(),
            last_insert_id: None,
            message: Some(format!("影响 {} 行", res.rows_affected())),
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
        let cols: Vec<String> = data.keys().map(|k| sql_identifier_pg(k)).collect();
        let vals: Vec<String> = data.values().map(json_to_sql_literal_pg).collect();
        let sql = format!(
            "INSERT INTO {} ({}) VALUES ({})",
            sql_identifier_pg(table),
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
            .map(|(k, v)| format!("{} = {}", sql_identifier_pg(k), json_to_sql_literal_pg(v)))
            .collect();
        let wheres: Vec<String> = pk
            .iter()
            .map(|(k, v)| format!("{} = {}", sql_identifier_pg(k), json_to_sql_literal_pg(v)))
            .collect();
        let sql = format!(
            "UPDATE {} SET {} WHERE {}",
            sql_identifier_pg(table),
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
            .map(|(k, v)| format!("{} = {}", sql_identifier_pg(k), json_to_sql_literal_pg(v)))
            .collect();
        let sql = format!(
            "DELETE FROM {} WHERE {}",
            sql_identifier_pg(table),
            wheres.join(" AND ")
        );
        self.execute(database, &sql).await
    }

    fn db_caps(&self) -> DbCaps {
        DbCaps {
            owner: true,
            template: true,
            encoding: true,
            collate: true,
            ctype: true,
            tablespace: true,
            conn_limit: true,
            allow_connect: true,
            is_template: true,
            comment: true,
            charset: false,
            extensions: true,
            // PG 里这些是建库时定死、之后改不了的
            immutable_on_alter: vec![
                "template".into(),
                "encoding".into(),
                "collate".into(),
                "ctype".into(),
                "tablespace".into(),
            ],
        }
    }

    async fn db_options(&self, database: &str) -> Result<DbOptions, String> {
        let pool = self.open_pool("").await?;
        let owners = text_col(
            &pool,
            "SELECT rolname FROM pg_roles WHERE rolsuper OR rolcanlogin ORDER BY rolname",
        )
        .await;
        let templates = text_col(
            &pool,
            "SELECT datname FROM pg_database WHERE datistemplate ORDER BY datname",
        )
        .await;
        let encodings = text_col(
            &pool,
            "SELECT DISTINCT pg_encoding_to_char(encoding) FROM pg_database ORDER BY 1",
        )
        .await;
        let collations = text_col(
            &pool,
            "SELECT DISTINCT collcollate FROM pg_collation WHERE collcollate IS NOT NULL \
             AND collcollate <> '' ORDER BY 1 LIMIT 400",
        )
        .await;
        let tablespaces = text_col(&pool, "SELECT spcname FROM pg_tablespace ORDER BY spcname").await;
        let current = if database.is_empty() {
            None
        } else {
            load_db_spec(&pool, database).await.ok()
        };
        Ok(DbOptions {
            caps: self.db_caps(),
            owners,
            templates,
            encodings,
            collations,
            tablespaces,
            charsets: vec![],
            collation_names: vec![],
            current,
        })
    }

    fn plan_database(&self, action: &str, spec: &DbSpec) -> Result<Vec<String>, String> {
        let name = spec.name.trim();
        if name.is_empty() {
            return Err("数据库名称不能为空".into());
        }
        let id = sql_identifier_pg(name);
        let mut out: Vec<String> = Vec::new();
        match action {
            "create" => {
                let mut with: Vec<String> = Vec::new();
                push_opt(&mut with, &spec.owner, |v| format!("OWNER = {}", sql_identifier_pg(v)));
                push_opt(&mut with, &spec.template, |v| {
                    format!("TEMPLATE = {}", sql_identifier_pg(v))
                });
                push_opt(&mut with, &spec.encoding, |v| {
                    format!("ENCODING = {}", pg_lit(v))
                });
                push_opt(&mut with, &spec.collate, |v| {
                    format!("LC_COLLATE = {}", pg_lit(v))
                });
                push_opt(&mut with, &spec.ctype, |v| {
                    format!("LC_CTYPE = {}", pg_lit(v))
                });
                push_opt(&mut with, &spec.tablespace, |v| {
                    if v == "pg_default" {
                        String::new()
                    } else {
                        format!("TABLESPACE = {}", sql_identifier_pg(v))
                    }
                });
                if let Some(n) = spec.conn_limit {
                    with.push(format!("CONNECTION LIMIT = {n}"));
                }
                let with = with.into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>();
                let mut stmt = format!("CREATE DATABASE {id}");
                if !with.is_empty() {
                    stmt.push_str(&format!(" WITH {}", with.join(" ")));
                }
                out.push(stmt);
                if let Some(false) = spec.allow_connect {
                    out.push(format!("ALTER DATABASE {id} WITH ALLOW_CONNECTIONS false"));
                }
                if let Some(true) = spec.is_template {
                    out.push(format!("ALTER DATABASE {id} WITH IS_TEMPLATE true"));
                }
                push_opt(&mut out, &spec.comment, |v| {
                    format!("COMMENT ON DATABASE {id} IS {}", pg_lit(v))
                });
            }
            "alter" => {
                let mut cur = id.clone();
                let to = spec.new_name.as_deref().unwrap_or("").trim();
                if !to.is_empty() && to != name {
                    out.push(format!(
                        "ALTER DATABASE {cur} RENAME TO {}",
                        sql_identifier_pg(to)
                    ));
                    cur = sql_identifier_pg(to);
                }
                push_opt(&mut out, &spec.owner, |v| {
                    format!("ALTER DATABASE {cur} OWNER TO {}", sql_identifier_pg(v))
                });
                if let Some(n) = spec.conn_limit {
                    out.push(format!("ALTER DATABASE {cur} WITH CONNECTION LIMIT {n}"));
                }
                if let Some(v) = spec.allow_connect {
                    out.push(format!("ALTER DATABASE {cur} WITH ALLOW_CONNECTIONS {v}"));
                }
                if let Some(v) = spec.is_template {
                    out.push(format!("ALTER DATABASE {cur} WITH IS_TEMPLATE {v}"));
                }
                let comment = spec.comment.as_deref().unwrap_or("").trim();
                out.push(format!(
                    "COMMENT ON DATABASE {cur} IS {}",
                    if comment.is_empty() {
                        "NULL".to_string()
                    } else {
                        pg_lit(comment)
                    }
                ));
            }
            "drop" => {
                if name == "template0" || name == "template1" {
                    return Err(format!("{name} 是模板库，拒绝删除"));
                }
                out.push(format!("DROP DATABASE {id}"));
            }
            other => return Err(format!("未知操作: {other}")),
        }
        Ok(out)
    }

    async fn drop_database(&self, name: &str) -> Result<Vec<String>, String> {
        let spec = DbSpec {
            name: name.to_string(),
            ..Default::default()
        };
        let stmts = self.plan_database("drop", &spec)?;
        match self.run_database_ddl(&stmts).await {
            Ok(()) => Ok(stmts),
            Err(first) => {
                // PG 13+：库上还有活跃连接（含本工具自己的连接池）时用 FORCE 再试一次
                let force = vec![format!(
                    "DROP DATABASE {} WITH (FORCE)",
                    sql_identifier_pg(name.trim())
                )];
                match self.run_database_ddl(&force).await {
                    Ok(()) => Ok(force),
                    Err(_) => Err(first),
                }
            }
        }
    }

    async fn list_extensions(&self, database: &str) -> Result<Vec<ExtInfo>, String> {
        let pool = self.open_pool(database).await?;
        let rows = sqlx::query(
            "SELECT pae.name, COALESCE(pae.default_version, ''), pe.extversion, \
                    COALESCE(pae.comment, ''), n.nspname \
             FROM pg_available_extensions pae \
             LEFT JOIN pg_extension pe ON pe.extname = pae.name \
             LEFT JOIN pg_namespace n ON n.oid = pe.extnamespace \
             ORDER BY (pe.extversion IS NULL), pae.name",
        )
        .fetch_all(&pool)
        .await
        .map_err(|e| e.to_string())?;
        Ok(rows
            .iter()
            .map(|r| ExtInfo {
                name: r.try_get(0).unwrap_or_default(),
                default_version: r.try_get(1).unwrap_or_default(),
                installed_version: r
                    .try_get::<Option<String>, _>(2)
                    .ok()
                    .flatten()
                    .filter(|s| !s.is_empty()),
                comment: r
                    .try_get::<Option<String>, _>(3)
                    .ok()
                    .flatten()
                    .filter(|s| !s.is_empty()),
                schema: r.try_get::<Option<String>, _>(4).ok().flatten(),
            })
            .collect())
    }

    fn plan_extension(&self, action: &str, name: &str) -> Result<Vec<String>, String> {
        let n = name.trim();
        if n.is_empty() {
            return Err("扩展名不能为空".into());
        }
        let id = sql_identifier_pg(n);
        match action {
            "create" => Ok(vec![format!("CREATE EXTENSION IF NOT EXISTS {id}")]),
            "drop" => Ok(vec![format!("DROP EXTENSION IF EXISTS {id}")]),
            other => Err(format!("未知操作: {other}")),
        }
    }
}

fn pg_lit(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

fn push_opt<F: Fn(&str) -> String>(out: &mut Vec<String>, v: &Option<String>, f: F) {
    if let Some(val) = v {
        let t = val.trim();
        if !t.is_empty() {
            out.push(f(t));
        }
    }
}

async fn text_col(pool: &sqlx::PgPool, sql: &str) -> Vec<String> {
    sqlx::query(sql)
        .fetch_all(pool)
        .await
        .unwrap_or_default()
        .iter()
        .filter_map(|r| r.try_get::<String, _>(0).ok())
        .collect()
}

async fn load_db_spec(pool: &sqlx::PgPool, database: &str) -> Result<DbSpec, String> {
    let row = sqlx::query(
        "SELECT d.datname, pg_get_userbyid(d.datdba), pg_encoding_to_char(d.encoding), \
                d.datcollate, d.datctype, COALESCE(t.spcname, 'pg_default'), \
                d.datconnlimit, d.datallowconn, d.datistemplate, \
                COALESCE(shobj_description(d.oid, 'pg_database'), '') \
         FROM pg_database d LEFT JOIN pg_tablespace t ON t.oid = d.dattablespace \
         WHERE d.datname = $1",
    )
    .bind(database)
    .fetch_optional(pool)
    .await
    .map_err(|e| e.to_string())?
    .ok_or_else(|| format!("未找到数据库 {database}"))?;

    let str_col = |i: usize| {
        row.try_get::<String, _>(i)
            .ok()
            .filter(|s| !s.is_empty())
    };
    Ok(DbSpec {
        name: row.try_get(0).unwrap_or_default(),
        new_name: None,
        owner: str_col(1),
        template: None,
        encoding: str_col(2),
        collate: str_col(3),
        ctype: str_col(4),
        tablespace: str_col(5),
        conn_limit: row.try_get::<i32, _>(6).ok().map(|v| v as i64),
        allow_connect: row.try_get::<bool, _>(7).ok(),
        is_template: row.try_get::<bool, _>(8).ok(),
        comment: str_col(9),
        charset: None,
        collation: None,
    })
}

/// 可视化建表：PostgreSQL 方言（自增用 serial / identity）
pub fn plan_create_table_pg(spec: &TableSpec) -> Result<Vec<String>, String> {
    let (cols, pks) = validate_table_spec(spec)?;
    let tname = spec.name.trim();
    let mut parts: Vec<String> = Vec::new();
    let mut comments: Vec<String> = Vec::new();
    for c in &cols {
        let cname = c.name.trim();
        let raw = c.data_type.as_deref().unwrap_or("").trim();
        let mut ty = raw.to_string();
        if c.auto_increment == Some(true) {
            ty = match raw.to_ascii_lowercase().as_str() {
                "smallint" | "int2" => "smallserial".into(),
                "integer" | "int" | "int4" => "serial".into(),
                "bigint" | "int8" => "bigserial".into(),
                other => format!("{other} generated by default as identity"),
            };
        }
        let mut p = format!("  {} {}", sql_identifier_pg(cname), ty);
        if c.nullable == Some(false) && c.auto_increment != Some(true) {
            p.push_str(" NOT NULL");
        }
        let d = c.default_value.as_deref().unwrap_or("").trim();
        if !d.is_empty() {
            p.push_str(&format!(" DEFAULT {}", pg_default_expr(d)));
        }
        let cm = c.comment.as_deref().unwrap_or("").trim();
        if !cm.is_empty() {
            comments.push(format!(
                "COMMENT ON COLUMN {}.{} IS '{}'",
                sql_identifier_pg(tname),
                sql_identifier_pg(cname),
                cm.replace('\'', "''")
            ));
        }
        parts.push(p);
    }
    if !pks.is_empty() {
        let list: Vec<String> = pks.iter().map(|p| sql_identifier_pg(p)).collect();
        parts.push(format!("  PRIMARY KEY ({})", list.join(", ")));
    }
    let mut out = vec![format!(
        "CREATE TABLE {} (\n{}\n)",
        sql_identifier_pg(tname),
        parts.join(",\n")
    )];
    let tc = spec.comment.as_deref().unwrap_or("").trim();
    if !tc.is_empty() {
        out.push(format!(
            "COMMENT ON TABLE {} IS '{}'",
            sql_identifier_pg(tname),
            tc.replace('\'', "''")
        ));
    }
    out.extend(comments);
    Ok(out)
}

fn pg_default_expr(d: &str) -> String {
    let up = d.to_ascii_uppercase();
    if d.eq_ignore_ascii_case("null")
        || up.starts_with("CURRENT_TIMESTAMP")
        || up.starts_with("NOW(")
        || up.starts_with("GEN_RANDOM_UUID(")
        || up.starts_with("(")
        || d.parse::<f64>().is_ok()
        || up == "TRUE"
        || up == "FALSE"
    {
        d.to_string()
    } else {
        format!("'{}'", d.replace('\'', "''"))
    }
}
