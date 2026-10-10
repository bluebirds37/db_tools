use super::*;
use crate::models::ConnectionConfig;
use serde_json::{json, Value};

/// ES 的元字段：不能进 `_source`，`_id` 只能当定位符用
const ES_META_FIELDS: &[&str] = &[
    "_id",
    "_index",
    "_score",
    "_type",
    "_version",
    "_source",
    "_seq_no",
    "_primary_term",
    "_field_names",
    "_routing",
];

fn drop_meta(data: &mut serde_json::Map<String, Value>) -> Vec<String> {
    let dropped = data
        .keys()
        .filter(|k| ES_META_FIELDS.contains(&k.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    for k in &dropped {
        data.remove(k);
    }
    dropped
}

/// 文档 id：表格里数字型 id 会被当成数字回传，这里统一还原成字符串
fn doc_id(map: &serde_json::Map<String, Value>) -> Option<String> {
    match map.get("_id") {
        Some(Value::String(s)) if !s.is_empty() => Some(s.clone()),
        Some(Value::Number(n)) => Some(n.to_string()),
        Some(Value::Bool(b)) => Some(b.to_string()),
        _ => None,
    }
}

pub struct EsSession {
    base: String,
    auth: Option<(String, String)>,
    client: reqwest::Client,
}

impl EsSession {
    pub async fn connect(config: &ConnectionConfig) -> Result<Self, String> {
        let host = config.host.clone().unwrap_or_else(|| "localhost".into());
        let port = config.port.unwrap_or(9200);
        let scheme = config
            .options
            .get("scheme")
            .cloned()
            .unwrap_or_else(|| "http".into());
        let base = format!("{scheme}://{host}:{port}");
        let auth = match (&config.username, &config.password) {
            (Some(u), Some(p)) if !u.is_empty() => Some((u.clone(), p.clone())),
            _ => None,
        };
        let client = reqwest::Client::builder()
            .connect_timeout(connect_timeout(config))
            .timeout(std::time::Duration::from_secs(15))
            .build()
            .map_err(|e| e.to_string())?;
        let s = Self {
            base,
            auth,
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
        if let Some((u, p)) = &self.auth {
            rb = rb.basic_auth(u, Some(p));
        }
        if let Some(b) = body {
            rb = rb.json(&b);
        }
        let resp = rb.send().await.map_err(|e| format!("ES 请求失败: {e}"))?;
        let status = resp.status();
        let text = resp.text().await.map_err(|e| e.to_string())?;
        let val: Value = serde_json::from_str(&text).unwrap_or(json!({ "raw": text }));
        if !status.is_success() {
            let msg = val
                .get("error")
                .map(|e| e.to_string())
                .unwrap_or(text.clone());
            return Err(format!("ES {status}: {msg}"));
        }
        Ok(val)
    }

    /// 写完对整个索引 refresh 一次。`?refresh=true` 只刷新写入涉及的分片，
    /// 多分片索引上会看不到其他分片里已写入但尚未刷新的历史文档。
    async fn refresh_index(&self, index: &str) {
        let _ = self
            .req(reqwest::Method::POST, &format!("/{index}/_refresh"), None)
            .await;
    }
}

#[async_trait]
impl DbSession for EsSession {
    fn kind(&self) -> DbKind {
        DbKind::Elasticsearch
    }

    async fn ping(&self) -> Result<(), String> {
        self.req(reqwest::Method::GET, "/_cluster/health", None)
            .await
            .map(|_| ())
    }

    async fn list_databases(&self) -> Result<Vec<String>, String> {
        // ES: treat aliases + all as "databases"
        Ok(vec!["_all".into()])
    }

    async fn list_tables(&self, _database: &str, _schema: Option<&str>) -> Result<Vec<TableInfo>, String> {
        let val = self
            .req(reqwest::Method::GET, "/_cat/indices?format=json&bytes=b", None)
            .await?;
        let arr = val.as_array().cloned().unwrap_or_default();
        Ok(arr
            .into_iter()
            .filter_map(|i| {
                let name = i.get("index")?.as_str()?.to_string();
                if name.starts_with('.') {
                    return None;
                }
                let docs = i
                    .get("docs.count")
                    .and_then(|v| v.as_str())
                    .and_then(|s| s.parse().ok());
                Some(TableInfo {
                    name,
                    schema: None,
                    table_type: Some("INDEX".into()),
                    comment: i
                        .get("health")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string()),
                    row_estimate: docs,
                    tablespace: None,
                })
            })
            .collect())
    }

    async fn list_columns(&self, _database: &str, _schema: Option<&str>, table: &str) -> Result<Vec<ColumnInfo>, String> {
        let val = self
            .req(
                reqwest::Method::GET,
                &format!("/{table}/_mapping"),
                None,
            )
            .await?;
        let mut cols = vec![ColumnInfo {
            name: "_id".into(),
            data_type: "keyword".into(),
            nullable: false,
            primary_key: true,
            default: None,
            comment: None,
            extra: None,
            max_length: None,
        }];
        if let Some(props) = val
            .pointer(&format!("/{table}/mappings/properties"))
            .or_else(|| val.pointer("/mappings/properties"))
        {
            if let Some(obj) = props.as_object() {
                for (k, v) in obj {
                    let t = v
                        .get("type")
                        .and_then(|x| x.as_str())
                        .unwrap_or("object")
                        .to_string();
                    cols.push(ColumnInfo {
                        name: k.clone(),
                        data_type: t,
                        nullable: true,
                        primary_key: false,
                        default: None,
                        comment: None,
                        extra: None,
                        max_length: None,
                    });
                }
            }
        }
        Ok(cols)
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
        let (index, query) = if let Some((idx, rest)) = s.split_once('|') {
            let q: Value = serde_json::from_str(rest.trim())
                .unwrap_or_else(|_| json!({ "query": { "match_all": {} } }));
            (idx.trim().to_string(), q)
        } else if s.contains('{') {
            // assume JSON body with index prefix optional
            let idx = "_all".to_string();
            let q: Value = serde_json::from_str(s)
                .map_err(|e| format!("查询 JSON 解析失败: {e}"))?;
            (idx, q)
        } else {
            (
                s.to_string(),
                json!({ "query": { "match_all": {} } }),
            )
        };

        let from = page.saturating_sub(1) * page_size;
        let body = json!({
            "from": from,
            "size": page_size,
            "query": query.get("query").cloned().unwrap_or(json!({"match_all": {}})),
            "sort": query.get("sort").cloned().unwrap_or(json!([{"_doc":"asc"}])),
        });

        let path = format!("/{index}/_search");
        let val = self
            .req(reqwest::Method::POST, &path, Some(body))
            .await?;

        let total = val
            .pointer("/hits/total/value")
            .and_then(|v| v.as_u64())
            .or_else(|| val.pointer("/hits/total").and_then(|v| v.as_u64()))
            .unwrap_or(0);

        let hits = val
            .pointer("/hits/hits")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        let mut columns = vec!["_id".to_string(), "_score".to_string()];
        let mut row_objs: Vec<serde_json::Map<String, Value>> = Vec::new();
        for h in &hits {
            let id = h.get("_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let score = h.get("_score").cloned().unwrap_or(Value::Null);
            let src = h
                .get("_source")
                .and_then(|v| v.as_object())
                .cloned()
                .unwrap_or_default();
            for k in src.keys() {
                if !columns.contains(k) {
                    columns.push(k.clone());
                }
            }
            let mut row = serde_json::Map::new();
            row.insert("_id".into(), Value::String(id));
            row.insert("_score".into(), score);
            for (k, v) in src {
                row.insert(k, v);
            }
            row_objs.push(row);
        }
        let rows: Vec<Vec<Value>> = row_objs
            .into_iter()
            .map(|mut m| {
                columns
                    .iter()
                    .map(|c| m.remove(c).unwrap_or(Value::Null))
                    .collect()
            })
            .collect();

        Ok(QueryPage {
            columns,
            rows,
            total,
            page,
            page_size,
            elapsed_ms: start.elapsed().as_millis() as u64,
            message: Some(format!("索引: {index}")),
        })
    }

    async fn execute(&self, _database: &str, sql: &str) -> Result<ExecResult, String> {
        // METHOD path [json_body]
        let (method, path, body) = crate::db::parse_rest_request(sql)?;
        let method = match method.as_str() {
            "GET" => reqwest::Method::GET,
            "POST" => reqwest::Method::POST,
            "PUT" => reqwest::Method::PUT,
            "DELETE" => reqwest::Method::DELETE,
            "HEAD" => reqwest::Method::HEAD,
            other => return Err(format!("不支持的方法: {other}")),
        };
        let val = self.req(method, &path, body).await?;
        Ok(ExecResult {
            affected_rows: 1,
            last_insert_id: None,
            message: Some(
                serde_json::to_string_pretty(&val).unwrap_or_else(|_| val.to_string()),
            ),
        })
    }

    async fn insert_row(
        &self,
        _database: &str,
        table: &str,
        data: serde_json::Map<String, Value>,
    ) -> Result<ExecResult, String> {
        let id = doc_id(&data);
        let mut src = data.clone();
        drop_meta(&mut src);
        let body = Value::Object(src);
        let path = match &id {
            Some(i) => format!("/{table}/_doc/{i}?refresh=true"),
            None => format!("/{table}/_doc?refresh=true"),
        };
        let val = self
            .req(reqwest::Method::POST, &path, Some(body))
            .await?;
        let created_id = val
            .get("_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        self.refresh_index(table).await;
        Ok(ExecResult {
            affected_rows: 1,
            last_insert_id: None,
            message: Some(format!("已写入文档 {created_id}")),
        })
    }

    async fn update_row(
        &self,
        _database: &str,
        table: &str,
        pk: serde_json::Map<String, Value>,
        data: serde_json::Map<String, Value>,
    ) -> Result<ExecResult, String> {
        let id = doc_id(&pk).ok_or("缺少 _id，无法定位文档")?;
        let mut doc = data.clone();
        let dropped = drop_meta(&mut doc);
        if doc.is_empty() {
            return Ok(ExecResult {
                affected_rows: 0,
                last_insert_id: None,
                message: Some(if dropped.is_empty() {
                    format!("文档 {id} 没有可更新的字段")
                } else {
                    format!(
                        "文档 {id} 没有可更新的字段（元字段 {} 不参与更新）",
                        dropped.join("、")
                    )
                }),
            });
        }
        let body = json!({ "doc": doc });
        let path = format!("/{table}/_update/{id}?refresh=true");
        self.req(reqwest::Method::POST, &path, Some(body)).await?;
        self.refresh_index(table).await;
        Ok(ExecResult {
            affected_rows: 1,
            last_insert_id: None,
            message: Some(format!(
                "已更新 {id}（{} 个字段）",
                doc.len()
            )),
        })
    }

    async fn delete_row(
        &self,
        _database: &str,
        table: &str,
        pk: serde_json::Map<String, Value>,
    ) -> Result<ExecResult, String> {
        let id = doc_id(&pk).ok_or("缺少 _id，无法定位文档")?;
        let path = format!("/{table}/_doc/{id}?refresh=true");
        self.req(reqwest::Method::DELETE, &path, None).await?;
        self.refresh_index(table).await;
        Ok(ExecResult {
            affected_rows: 1,
            last_insert_id: None,
            message: Some(format!("已删除 {id}")),
        })
    }
}
