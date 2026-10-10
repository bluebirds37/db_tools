use super::*;
use crate::models::ConnectionConfig;
use futures::TryStreamExt;
use ::mongodb::bson::{doc, Bson, Document};
use ::mongodb::options::ClientOptions;
use ::mongodb::{Client, Database};

pub struct MongoSession {
    client: Client,
    default_db: String,
}

impl MongoSession {
    pub async fn connect(config: &ConnectionConfig) -> Result<Self, String> {
        let host = config.host.clone().unwrap_or_else(|| "localhost".into());
        let port = config.port.unwrap_or(27017);
        let user = config.username.clone().unwrap_or_default();
        let pass = config.password.clone().unwrap_or_default();
        let auth_source = config
            .options
            .get("authSource")
            .cloned()
            .unwrap_or_else(|| "admin".into());
        let default_db = config
            .database
            .clone()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "test".into());

        let mut url = String::from("mongodb://");
        if !user.is_empty() {
            url.push_str(&user);
            url.push(':');
            url.push_str(&pass);
            url.push('@');
        }
        url.push_str(&format!("{}:{}/{}", host, port, default_db));
        if !user.is_empty() {
            url.push_str(&format!("?authSource={auth_source}"));
        }

        let mut options = ClientOptions::parse(&url)
            .await
            .map_err(|e| format!("Mongo 连接串错误: {e}"))?;
        options.app_name = Some("db_tools".into());
        options.server_selection_timeout = Some(connect_timeout(config));
        let client = Client::with_options(options).map_err(|e| e.to_string())?;
        client
            .database("admin")
            .run_command(doc! { "ping": 1 })
            .await
            .map_err(|e| format!("Mongo 连接失败: {e}"))?;
        Ok(Self {
            client,
            default_db,
        })
    }

    fn db(&self, name: &str) -> Database {
        let n = if name.is_empty() {
            &self.default_db
        } else {
            name
        };
        self.client.database(n)
    }
}

fn bson_to_json(v: Bson) -> serde_json::Value {
    match v {
        Bson::Double(d) => serde_json::Number::from_f64(d)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null),
        Bson::String(s) => serde_json::Value::String(s),
        Bson::Int32(i) => serde_json::Value::from(i),
        Bson::Int64(i) => serde_json::Value::from(i),
        Bson::Boolean(b) => serde_json::Value::Bool(b),
        Bson::Null | Bson::Undefined => serde_json::Value::Null,
        Bson::ObjectId(oid) => serde_json::Value::String(oid.to_hex()),
        Bson::DateTime(dt) => {
            let millis = dt.timestamp_millis();
            match chrono::DateTime::from_timestamp_millis(millis) {
                Some(t) => serde_json::Value::String(t.to_rfc3339()),
                None => serde_json::Value::from(millis),
            }
        }
        Bson::Array(arr) => serde_json::Value::Array(arr.into_iter().map(bson_to_json).collect()),
        Bson::Document(doc) => {
            let mut map = serde_json::Map::new();
            for (k, v) in doc {
                map.insert(k, bson_to_json(v));
            }
            serde_json::Value::Object(map)
        }
        other => serde_json::Value::String(format!("{other:?}")),
    }
}

fn json_to_bson(v: &serde_json::Value) -> Bson {
    match v {
        serde_json::Value::Null => Bson::Null,
        serde_json::Value::Bool(b) => Bson::Boolean(*b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Bson::Int64(i)
            } else {
                Bson::Double(n.as_f64().unwrap_or(0.0))
            }
        }
        serde_json::Value::String(s) => {
            if s.len() == 24 {
                if let Ok(oid) = ::mongodb::bson::oid::ObjectId::parse_str(s) {
                    return Bson::ObjectId(oid);
                }
            }
            Bson::String(s.clone())
        }
        serde_json::Value::Array(a) => Bson::Array(a.iter().map(json_to_bson).collect()),
        serde_json::Value::Object(o) => {
            let mut d = Document::new();
            for (k, v) in o {
                d.insert(k, json_to_bson(v));
            }
            Bson::Document(d)
        }
    }
}

#[async_trait]
impl DbSession for MongoSession {
    fn kind(&self) -> DbKind {
        DbKind::Mongodb
    }

    async fn ping(&self) -> Result<(), String> {
        self.client
            .database("admin")
            .run_command(doc! { "ping": 1 })
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    async fn list_databases(&self) -> Result<Vec<String>, String> {
        self.client
            .list_database_names()
            .await
            .map_err(|e| e.to_string())
    }

    async fn list_tables(&self, database: &str, _schema: Option<&str>) -> Result<Vec<TableInfo>, String> {
        let db = self.db(database);
        let names = db.list_collection_names().await.map_err(|e| e.to_string())?;
        let mut result = Vec::new();
        for n in names {
            let estimate = db
                .collection::<Document>(&n)
                .estimated_document_count()
                .await
                .ok();
            result.push(TableInfo {
                name: n,
                schema: Some(database.to_string()),
                table_type: Some("COLLECTION".into()),
                comment: None,
                row_estimate: estimate.map(|v| v as i64),
                tablespace: None,
            });
        }
        result.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(result)
    }

    async fn list_columns(&self, database: &str, _schema: Option<&str>, table: &str) -> Result<Vec<ColumnInfo>, String> {
        let db = self.db(database);
        let coll = db.collection::<Document>(table);
        let sample: Vec<Document> = coll
            .find(doc! {})
            .limit(50)
            .await
            .map_err(|e| e.to_string())?
            .try_collect()
            .await
            .map_err(|e| e.to_string())?;
        let mut fields: std::collections::BTreeMap<String, String> = Default::default();
        for d in &sample {
            for (k, v) in d {
                let t = match v {
                    Bson::ObjectId(_) => "objectId",
                    Bson::String(_) => "string",
                    Bson::Int32(_) | Bson::Int64(_) => "int",
                    Bson::Double(_) => "double",
                    Bson::Boolean(_) => "bool",
                    Bson::Array(_) => "array",
                    Bson::Document(_) => "object",
                    Bson::DateTime(_) => "date",
                    Bson::Null => "null",
                    _ => "other",
                };
                fields.entry(k.clone()).or_insert_with(|| t.into());
            }
        }
        if fields.is_empty() {
            fields.insert("_id".into(), "objectId".into());
        }
        Ok(fields
            .into_iter()
            .map(|(name, data_type)| ColumnInfo {
                primary_key: name == "_id",
                nullable: true,
                comment: None,
                default: None,
                extra: None,
                max_length: None,
                name,
                data_type,
            })
            .collect())
    }

    async fn list_indexes(&self, database: &str, _schema: Option<&str>, table: &str) -> Result<Vec<IndexInfo>, String> {
        use ::mongodb::bson::to_document;
        let db = self.db(database);
        let coll = db.collection::<Document>(table);
        let cursor = coll.list_indexes().await.map_err(|e| e.to_string())?;
        let indexes: Vec<::mongodb::IndexModel> =
            cursor.try_collect().await.map_err(|e| e.to_string())?;
        let mut result = Vec::new();
        for model in indexes {
            let raw = to_document(&model).map_err(|e| e.to_string())?;
            let name = raw.get_str("name").unwrap_or("idx").to_string();
            let mut columns = vec![];
            if let Ok(key) = raw.get_document("key") {
                for (k, _) in key.iter() {
                    columns.push(k.clone());
                }
            }
            result.push(IndexInfo {
                primary: name == "_id_",
                unique: raw.get_bool("unique").unwrap_or(false),
                name,
                columns,
            });
        }
        Ok(result)
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
        let (coll_name, filter) = parse_mongo_query(sql);
        let db = self.db(database);
        let coll = db.collection::<Document>(&coll_name);
        let total = coll
            .count_documents(filter.clone())
            .await
            .map_err(|e| e.to_string())? as u64;
        let skip = page.saturating_sub(1) * page_size;
        let cursor = coll
            .find(filter)
            .skip(skip)
            .limit(page_size as i64)
            .await
            .map_err(|e| e.to_string())?;
        let docs: Vec<Document> = cursor.try_collect().await.map_err(|e| e.to_string())?;

        let mut columns = vec!["_id".to_string()];
        let mut rows = Vec::new();
        for d in &docs {
            let mut map = serde_json::Map::new();
            for (k, v) in d {
                if !columns.contains(k) {
                    columns.push(k.clone());
                }
                map.insert(k.clone(), bson_to_json(v.clone()));
            }
            rows.push(map);
        }
        let aligned: Vec<Vec<serde_json::Value>> = rows
            .into_iter()
            .map(|mut m| {
                columns
                    .iter()
                    .map(|c| m.remove(c).unwrap_or(serde_json::Value::Null))
                    .collect()
            })
            .collect();

        Ok(QueryPage {
            columns,
            rows: aligned,
            total,
            page,
            page_size,
            elapsed_ms: start.elapsed().as_millis() as u64,
            message: Some(format!("集合: {coll_name}")),
        })
    }

    async fn execute(&self, database: &str, sql: &str) -> Result<ExecResult, String> {
        let s = sql.trim();
        if let Some(rest) = s.strip_prefix("db.") {
            if let Some((_coll, cmd)) = rest.split_once('.') {
                let db = self.db(database);
                let filter: Document =
                    serde_json::from_str(cmd.trim().trim_end_matches(';')).unwrap_or_default();
                let res = db.run_command(filter).await.map_err(|e| e.to_string())?;
                return Ok(ExecResult {
                    affected_rows: 1,
                    last_insert_id: None,
                    message: Some(format!("{res:?}")),
                });
            }
        }
        Err("Mongo 请使用表格视图进行增删改查，或输入 db.xxx.{json}".into())
    }

    async fn insert_row(
        &self,
        database: &str,
        table: &str,
        data: serde_json::Map<String, serde_json::Value>,
    ) -> Result<ExecResult, String> {
        let mut doc = Document::new();
        for (k, v) in &data {
            if k == "_id" && v.is_null() {
                continue;
            }
            doc.insert(k, json_to_bson(v));
        }
        let db = self.db(database);
        let coll = db.collection::<Document>(table);
        let res = coll.insert_one(doc).await.map_err(|e| e.to_string())?;
        Ok(ExecResult {
            affected_rows: 1,
            last_insert_id: None,
            message: Some(format!("已插入 _id={}", bson_to_json(res.inserted_id))),
        })
    }

    async fn update_row(
        &self,
        database: &str,
        table: &str,
        pk: serde_json::Map<String, serde_json::Value>,
        data: serde_json::Map<String, serde_json::Value>,
    ) -> Result<ExecResult, String> {
        let mut filter = Document::new();
        for (k, v) in &pk {
            filter.insert(k, json_to_bson(v));
        }
        let mut set_doc = Document::new();
        for (k, v) in &data {
            if k != "_id" {
                set_doc.insert(k, json_to_bson(v));
            }
        }
        let db = self.db(database);
        let coll = db.collection::<Document>(table);
        let res = coll
            .update_one(filter, doc! { "$set": set_doc })
            .await
            .map_err(|e| e.to_string())?;
        Ok(ExecResult {
            affected_rows: res.modified_count as u64,
            last_insert_id: None,
            message: Some(format!("修改 {} 条", res.modified_count)),
        })
    }

    async fn delete_row(
        &self,
        database: &str,
        table: &str,
        pk: serde_json::Map<String, serde_json::Value>,
    ) -> Result<ExecResult, String> {
        let mut filter = Document::new();
        for (k, v) in &pk {
            filter.insert(k, json_to_bson(v));
        }
        let db = self.db(database);
        let coll = db.collection::<Document>(table);
        let res = coll.delete_one(filter).await.map_err(|e| e.to_string())?;
        Ok(ExecResult {
            affected_rows: res.deleted_count as u64,
            last_insert_id: None,
            message: Some(format!("删除 {} 条", res.deleted_count)),
        })
    }
}

fn parse_mongo_query(sql: &str) -> (String, Document) {
    let s = sql.trim().trim_end_matches(';');
    if s.is_empty() {
        return ("test".into(), doc! {});
    }
    if let Some((name, filter)) = s.split_once('|') {
        let f: Document = serde_json::from_str(filter.trim()).unwrap_or_else(|_| doc! {});
        return (name.trim().to_string(), f);
    }
    (s.to_string(), doc! {})
}
