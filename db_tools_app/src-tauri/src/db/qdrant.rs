use super::*;
use crate::models::ConnectionConfig;
use serde_json::{json, Value};

pub struct QdrantSession {
    base: String,
    api_key: Option<String>,
    client: reqwest::Client,
}

impl QdrantSession {
    pub async fn connect(config: &ConnectionConfig) -> Result<Self, String> {
        let host = config.host.clone().unwrap_or_else(|| "localhost".into());
        let port = config.port.unwrap_or(6333);
        let base = format!("http://{host}:{port}");
        let api_key = config
            .api_key_ref()
            .filter(|s| !s.is_empty())
            .or_else(|| config.options.get("api_key").cloned());
        let client = reqwest::Client::builder()
            .connect_timeout(connect_timeout(config))
            .timeout(std::time::Duration::from_secs(15))
            .build()
            .map_err(|e| e.to_string())?;
        let s = Self {
            base,
            api_key,
            client,
        };
        s.ping().await?;
        Ok(s)
    }

    async fn req(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<Value>,
    ) -> Result<Value, String> {
        let url = format!("{}{}", self.base, path);
        let mut rb = self.client.request(method, &url);
        if let Some(k) = &self.api_key {
            rb = rb.header("api-key", k);
        }
        if let Some(b) = body {
            rb = rb.json(&b);
        }
        let resp = rb.send().await.map_err(|e| format!("Qdrant 请求失败: {e}"))?;
        let status = resp.status();
        let text = resp.text().await.map_err(|e| e.to_string())?;
        let val: Value = serde_json::from_str(&text).unwrap_or(json!({ "raw": text }));
        if !status.is_success() {
            return Err(format!("Qdrant {status}: {text}"));
        }
        Ok(val)
    }
}

// helper trait extension on ConnectionConfig
trait ConfigExt {
    fn api_key_ref(&self) -> Option<String>;
}
impl ConfigExt for ConnectionConfig {
    fn api_key_ref(&self) -> Option<String> {
        self.options.get("api_key").cloned()
    }
}

#[async_trait]
impl DbSession for QdrantSession {
    fn kind(&self) -> DbKind {
        DbKind::Qdrant
    }

    async fn ping(&self) -> Result<(), String> {
        self.req(reqwest::Method::GET, "/readyz", None)
            .await
            .map(|_| ())
    }

    async fn list_databases(&self) -> Result<Vec<String>, String> {
        Ok(vec!["default".into()])
    }

    async fn list_tables(&self, _database: &str, _schema: Option<&str>) -> Result<Vec<TableInfo>, String> {
        let val = self
            .req(reqwest::Method::GET, "/collections", None)
            .await?;
        let arr = val
            .pointer("/result/collections")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        Ok(arr
            .into_iter()
            .filter_map(|c| {
                let name = c.get("name")?.as_str()?.to_string();
                Some(TableInfo {
                    name,
                    schema: None,
                    table_type: Some("COLLECTION".into()),
                    comment: Some("向量集合".into()),
                    row_estimate: None,
                    tablespace: None,
                })
            })
            .collect())
    }

    async fn list_columns(&self, _database: &str, _schema: Option<&str>, table: &str) -> Result<Vec<ColumnInfo>, String> {
        let val = self
            .req(
                reqwest::Method::GET,
                &format!("/collections/{table}"),
                None,
            )
            .await?;
        let dim = val
            .pointer("/result/config/vectors/size")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        Ok(vec![
            ColumnInfo {
                name: "id".into(),
                data_type: "point_id".into(),
                nullable: false,
                primary_key: true,
                default: None,
                comment: None,
                extra: None,
                max_length: None,
            },
            ColumnInfo {
                name: "vector".into(),
                data_type: format!("vector[{dim}]"),
                nullable: false,
                primary_key: false,
                default: None,
                comment: Some("稠密向量".into()),
                extra: None,
                max_length: Some(dim),
            },
            ColumnInfo {
                name: "payload".into(),
                data_type: "json".into(),
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
        Ok(vec![])
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
        _database: &str,
        sql: &str,
        page: u64,
        page_size: u64,
    ) -> Result<QueryPage, String> {
        let start = std::time::Instant::now();
        let s = sql.trim();
        // collection  or  collection|scroll filter json
        let (coll, filter) = if let Some((c, f)) = s.split_once('|') {
            let fv: Value = serde_json::from_str(f.trim()).unwrap_or(Value::Null);
            (c.trim().to_string(), fv)
        } else {
            (s.to_string(), Value::Null)
        };
        if coll.is_empty() {
            return Err("请指定集合名".into());
        }
        let offset = page.saturating_sub(1) * page_size;
        let mut body = json!({
            "limit": page_size,
            "offset": offset,
        });
        if !filter.is_null() {
            body["filter"] = filter;
        }
        let val = self
            .req(
                reqwest::Method::POST,
                &format!("/collections/{coll}/points/scroll"),
                Some(body),
            )
            .await?;
        let points = val
            .pointer("/result/points")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        let mut columns = vec![
            "id".to_string(),
            "score".to_string(),
            "vector_dim".to_string(),
            "payload".to_string(),
        ];
        let mut rows = Vec::new();
        for p in &points {
            let id = p.get("id").cloned().unwrap_or(Value::Null);
            let score = p.get("score").cloned().unwrap_or(Value::Null);
            let vec_arr = p.get("vector").and_then(|v| v.as_array());
            let dim = vec_arr.map(|a| a.len()).unwrap_or(0);
            let payload = p.get("payload").cloned().unwrap_or(Value::Null);
            // expand payload keys
            if let Some(obj) = payload.as_object() {
                for k in obj.keys() {
                    let col = format!("payload.{k}");
                    if !columns.contains(&col) {
                        columns.push(col);
                    }
                }
            }
            rows.push(vec![
                id,
                score,
                Value::from(dim as i64),
                payload.clone(),
            ]);
        }
        // realign with payload expansion
        let mut aligned = Vec::new();
        for (i, p) in points.iter().enumerate() {
            let mut row = rows[i].clone();
            let payload = p.get("payload").cloned().unwrap_or(Value::Null);
            for c in columns.iter().skip(4) {
                let key = c.strip_prefix("payload.").unwrap_or(c);
                let v = payload.get(key).cloned().unwrap_or(Value::Null);
                row.push(v);
            }
            aligned.push(row);
        }

        // approximate total via collection info
        let info = self
            .req(
                reqwest::Method::GET,
                &format!("/collections/{coll}"),
                None,
            )
            .await
            .unwrap_or_default();
        let total = info
            .pointer("/result/points_count")
            .and_then(|v| v.as_u64())
            .unwrap_or(aligned.len() as u64);

        Ok(QueryPage {
            columns,
            rows: aligned,
            total,
            page,
            page_size,
            elapsed_ms: start.elapsed().as_millis() as u64,
            message: Some(format!("集合: {coll}")),
        })
    }

    async fn execute(&self, _database: &str, sql: &str) -> Result<ExecResult, String> {
        // METHOD path [json]
        let (method, path, body) = crate::db::parse_rest_request(sql)?;
        let method = match method.as_str() {
            "GET" => reqwest::Method::GET,
            "POST" => reqwest::Method::POST,
            "PUT" => reqwest::Method::PUT,
            "DELETE" => reqwest::Method::DELETE,
            other => return Err(format!("不支持的方法: {other}")),
        };
        let val = self.req(method, &path, body).await?;
        Ok(ExecResult {
            affected_rows: 1,
            last_insert_id: None,
            message: Some(serde_json::to_string_pretty(&val).unwrap_or_default()),
        })
    }

    async fn insert_row(
        &self,
        _database: &str,
        table: &str,
        data: serde_json::Map<String, Value>,
    ) -> Result<ExecResult, String> {
        let id = data.get("id").cloned().unwrap_or(Value::Null);
        let vector = data
            .get("vector")
            .cloned()
            .ok_or("缺少 vector 字段")?;
        let mut payload = data.get("payload").cloned().unwrap_or(json!({}));
        if payload.is_null() {
            payload = json!({});
        }
        let body = json!({
            "points": [{
                "id": id,
                "vector": vector,
                "payload": payload
            }]
        });
        let val = self
            .req(
                reqwest::Method::PUT,
                &format!("/collections/{table}/points"),
                Some(body),
            )
            .await?;
        Ok(ExecResult {
            affected_rows: 1,
            last_insert_id: None,
            message: Some(val.to_string()),
        })
    }

    async fn update_row(
        &self,
        database: &str,
        table: &str,
        _pk: serde_json::Map<String, Value>,
        data: serde_json::Map<String, Value>,
    ) -> Result<ExecResult, String> {
        self.insert_row(database, table, data).await
    }

    async fn delete_row(
        &self,
        _database: &str,
        table: &str,
        pk: serde_json::Map<String, Value>,
    ) -> Result<ExecResult, String> {
        let id = pk.get("id").cloned().ok_or("缺少 id")?;
        let body = json!({ "points": [id] });
        self.req(
            reqwest::Method::POST,
            &format!("/collections/{table}/points/delete"),
            Some(body),
        )
        .await?;
        Ok(ExecResult {
            affected_rows: 1,
            last_insert_id: None,
            message: Some("已删除向量点".into()),
        })
    }
}
