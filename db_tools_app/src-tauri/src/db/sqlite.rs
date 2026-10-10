use super::*;
use crate::models::ConnectionConfig;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{Column, Row};
use std::str::FromStr;

pub struct SqliteSession {
    pool: sqlx::SqlitePool,
    path: String,
}

impl SqliteSession {
    pub async fn connect(config: &ConnectionConfig) -> Result<Self, String> {
        let path = config
            .options
            .get("path")
            .cloned()
            .or_else(|| config.database.clone())
            .unwrap_or_else(|| ":memory:".into());
        if path != ":memory:" {
            if let Some(parent) = std::path::Path::new(&path).parent() {
                let _ = std::fs::create_dir_all(parent);
            }
        }
        let options = SqliteConnectOptions::from_str(&format!("sqlite://{}", path))
            .map_err(|e| e.to_string())?
            .create_if_missing(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(4)
            .connect_with(options)
            .await
            .map_err(|e| format!("SQLite 打开失败: {e}"))?;
        Ok(Self { pool, path })
    }
}

#[async_trait]
impl DbSession for SqliteSession {
    fn kind(&self) -> DbKind {
        DbKind::Sqlite
    }

    fn plan_create_table(&self, spec: &TableSpec) -> Result<Vec<String>, String> {
        plan_create_table_sqlite(spec)
    }

    async fn ping(&self) -> Result<(), String> {
        sqlx::query("SELECT 1")
            .execute(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    async fn list_databases(&self) -> Result<Vec<String>, String> {
        Ok(vec![self.path.clone()])
    }

    async fn list_tables(&self, _database: &str, _schema: Option<&str>) -> Result<Vec<TableInfo>, String> {
        let rows = sqlx::query(
            "SELECT name, type FROM sqlite_master \
             WHERE type IN ('table','view') AND name NOT LIKE 'sqlite_%' \
             ORDER BY name",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| e.to_string())?;
        Ok(rows
            .iter()
            .map(|r| {
                let t: String = r.try_get(1).unwrap_or_default();
                TableInfo {
                    name: r.try_get(0).unwrap_or_default(),
                    schema: None,
                    table_type: Some(if t == "view" { "VIEW" } else { "TABLE" }.into()),
                    comment: None,
                    row_estimate: None,
                    tablespace: None,
                }
            })
            .collect())
    }

    async fn list_columns(&self, _database: &str, _schema: Option<&str>, table: &str) -> Result<Vec<ColumnInfo>, String> {
        let rows = sqlx::query(&format!("PRAGMA table_info({})", sql_identifier(table)))
            .fetch_all(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
        Ok(rows
            .iter()
            .map(|r| {
                let notnull: i64 = r.try_get(3).unwrap_or(0);
                let pk: i64 = r.try_get(5).unwrap_or(0);
                ColumnInfo {
                    name: r.try_get(1).unwrap_or_default(),
                    data_type: r.try_get(2).unwrap_or_else(|_| "TEXT".into()),
                    nullable: notnull == 0,
                    primary_key: pk > 0,
                    default: r.try_get(4).ok(),
                    comment: None,
                    extra: if pk > 0 { Some("PRIMARY KEY".into()) } else { None },
                    max_length: None,
                }
            })
            .collect())
    }

    async fn list_indexes(&self, _database: &str, _schema: Option<&str>, table: &str) -> Result<Vec<IndexInfo>, String> {
        let rows = sqlx::query(&format!("PRAGMA index_list({})", sql_identifier(table)))
            .fetch_all(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
        let mut result = Vec::new();
        for r in &rows {
            let name: String = r.try_get(1).unwrap_or_default();
            let unique: i64 = r.try_get(2).unwrap_or(0);
            let origin: String = r.try_get(3).unwrap_or_default();
            let info_rows =
                sqlx::query(&format!("PRAGMA index_info({})", sql_identifier(&name)))
                    .fetch_all(&self.pool)
                    .await
                    .unwrap_or_default();
            let cols: Vec<String> = info_rows
                .iter()
                .map(|ir| ir.try_get(2).unwrap_or_default())
                .collect();
            result.push(IndexInfo {
                name,
                columns: cols,
                unique: unique == 1,
                primary: origin == "pk",
            });
        }
        Ok(result)
    }

    async fn list_foreign_keys(
        &self,
        _database: &str,
        _schema: Option<&str>,
        table: &str,
    ) -> Result<Vec<ForeignKeyInfo>, String> {
        let rows = sqlx::query(&format!("PRAGMA foreign_key_list({})", sql_identifier(table)))
            .fetch_all(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
        Ok(rows
            .iter()
            .map(|r| ForeignKeyInfo {
                name: format!("fk_{}_{}", table, r.try_get::<i64, _>(0).unwrap_or(0)),
                column: r.try_get(3).unwrap_or_default(),
                ref_table: r.try_get(2).unwrap_or_default(),
                ref_column: r.try_get(4).unwrap_or_default(),
            })
            .collect())
    }

    async fn query_page(
        &self,
        _database: &str,
        sql: &str,
        page: u64,
        page_size: u64,
    ) -> Result<QueryPage, String> {
        let start = std::time::Instant::now();
        let trimmed = sql.trim_end_matches([';', ' ', '\n', '\r', '\t']);
        let offset = page.saturating_sub(1) * page_size;
        let paged = format!(
            "SELECT * FROM ({}) LIMIT {} OFFSET {}",
            trimmed, page_size, offset
        );
        let rows = sqlx::query(&paged)
            .fetch_all(&self.pool)
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
                    .map(|(i, _)| row_value_sqlite(r, i))
                    .collect()
            })
            .collect();
        let count_sql = format!("SELECT COUNT(*) FROM ({})", trimmed);
        let total: u64 = match sqlx::query(&count_sql).fetch_one(&self.pool).await {
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
            message: None,
        })
    }

    async fn execute(&self, _database: &str, sql: &str) -> Result<ExecResult, String> {
        let res = sqlx::raw_sql(sql)
            .execute(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
        Ok(ExecResult {
            affected_rows: res.rows_affected(),
            last_insert_id: {
                let id = res.last_insert_rowid();
                if id == 0 {
                    None
                } else {
                    Some(id)
                }
            },
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
}

fn row_value_sqlite(row: &sqlx::sqlite::SqliteRow, idx: usize) -> serde_json::Value {
    use sqlx::TypeInfo;
    let type_name = row.columns()[idx].type_info().name().to_uppercase();
    if type_name.contains("INT") {
        return match row.try_get::<Option<i64>, _>(idx) {
            Ok(Some(n)) => serde_json::Value::from(n),
            _ => serde_json::Value::Null,
        };
    }
    if type_name.contains("REAL") || type_name.contains("FLOA") || type_name.contains("DOUB") {
        return match row.try_get::<Option<f64>, _>(idx) {
            Ok(Some(n)) => serde_json::Number::from_f64(n)
                .map(serde_json::Value::Number)
                .unwrap_or(serde_json::Value::Null),
            _ => serde_json::Value::Null,
        };
    }
    if type_name.contains("BLOB") {
        return match row.try_get::<Option<Vec<u8>>, _>(idx) {
            Ok(Some(b)) => serde_json::Value::String(format!("<blob {} bytes>", b.len())),
            _ => serde_json::Value::Null,
        };
    }
    match row.try_get::<Option<String>, _>(idx) {
        Ok(Some(s)) => serde_json::Value::String(s),
        _ => match row.try_get::<Option<i64>, _>(idx) {
            Ok(Some(n)) => serde_json::Value::from(n),
            _ => match row.try_get::<Option<f64>, _>(idx) {
                Ok(Some(n)) => serde_json::Number::from_f64(n)
                    .map(serde_json::Value::Number)
                    .unwrap_or(serde_json::Value::Null),
                _ => serde_json::Value::Null,
            },
        },
    }
}

/// 可视化建表：SQLite 方言（无注释、无库级字符集）
pub fn plan_create_table_sqlite(spec: &TableSpec) -> Result<Vec<String>, String> {
    let (cols, pks) = validate_table_spec(spec)?;
    let tname = spec.name.trim();
    let inline_pk = if pks.len() == 1 {
        let only = &pks[0];
        cols.iter()
            .find(|c| c.name.trim() == only && c.auto_increment == Some(true))
            .map(|_| only.clone())
    } else {
        None
    };
    let mut parts: Vec<String> = Vec::new();
    for c in &cols {
        let name = c.name.trim();
        let ty = c.data_type.as_deref().unwrap_or("TEXT").trim();
        // SQLite 的自增主键必须是 INTEGER PRIMARY KEY（rowid 别名），BIGINT 会直接报错
        let ty = if inline_pk.as_deref() == Some(name) {
            "INTEGER"
        } else {
            ty
        };
        let mut p = format!("  {} {}", sql_identifier_pg(name), ty);
        if inline_pk.as_deref() == Some(name) {
            p.push_str(" PRIMARY KEY AUTOINCREMENT");
        } else {
            if c.nullable == Some(false) {
                p.push_str(" NOT NULL");
            }
            let d = c.default_value.as_deref().unwrap_or("").trim();
            if !d.is_empty() {
                p.push_str(&format!(" DEFAULT {}", sqlite_default_expr(d)));
            }
        }
        parts.push(p);
    }
    if inline_pk.is_none() && !pks.is_empty() {
        let list: Vec<String> = pks.iter().map(|p| sql_identifier_pg(p)).collect();
        parts.push(format!("  PRIMARY KEY ({})", list.join(", ")));
    }
    Ok(vec![format!(
        "CREATE TABLE {} (\n{}\n)",
        sql_identifier_pg(tname),
        parts.join(",\n")
    )])
}

fn sqlite_default_expr(d: &str) -> String {
    let up = d.to_ascii_uppercase();
    if d.eq_ignore_ascii_case("null")
        || up == "CURRENT_TIMESTAMP"
        || up.starts_with("(")
        || d.parse::<f64>().is_ok()
    {
        d.to_string()
    } else {
        format!("'{}'", d.replace('\'', "''"))
    }
}
