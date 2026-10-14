// Full feature smoke test: cargo run --example full_test
use agentdb_app_lib::db;
use agentdb_app_lib::models::{AiSettings, ConnectionConfig, DbKind};
use serde_json::json;
use std::collections::HashMap;

fn conn(
    id: &str,
    kind: DbKind,
    host: &str,
    port: u16,
    user: Option<&str>,
    pass: Option<&str>,
) -> ConnectionConfig {
    ConnectionConfig {
        id: id.into(),
        name: id.into(),
        kind,
        host: Some(host.into()),
        port: Some(port),
        username: user.map(|s| s.to_string()),
        password: pass.map(|s| s.to_string()),
        database: None,
        options: HashMap::new(),
        color: None,
        created_at: None,
        updated_at: None,
    }
}

async fn check(name: &str, fut: impl std::future::Future<Output = Result<(), String>>) -> bool {
    match fut.await {
        Ok(()) => {
            println!("PASS  {name}");
            true
        }
        Err(e) => {
            println!("FAIL  {name}: {e}");
            false
        }
    }
}

async fn exercise_sql(c: &ConnectionConfig, db_name: &str) -> Result<(), String> {
    let s = db::open_session(c).await?;
    s.ping().await?;
    let mut dbs = s.list_databases().await?;
    let database = if c.kind == DbKind::Mysql {
        if !dbs.iter().any(|d| d == "agentdb_test") {
            s.execute("", "CREATE DATABASE IF NOT EXISTS agentdb_test")
                .await?;
            dbs.push("agentdb_test".into());
        }
        "agentdb_test".to_string()
    } else if db_name.is_empty() {
        dbs.first().cloned().unwrap_or_default()
    } else {
        db_name.to_string()
    };
    let _tables = s.list_tables(&database, None).await?;
    let ddl = match c.kind {
        DbKind::Postgres | DbKind::Pgvector => {
            "CREATE TABLE IF NOT EXISTS agentdb_smoke (id SERIAL PRIMARY KEY, name TEXT, n INT)"
        }
        DbKind::Sqlite => {
            "CREATE TABLE IF NOT EXISTS agentdb_smoke (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT, n INTEGER)"
        }
        _ => "CREATE TABLE IF NOT EXISTS agentdb_smoke (id INT AUTO_INCREMENT PRIMARY KEY, name VARCHAR(64), n INT)",
    };
    s.execute(&database, ddl).await?;
    let ins = json!({"name": "smoke", "n": 1});
    s.insert_row(
        &database,
        "agentdb_smoke",
        ins.as_object().cloned().unwrap_or_default(),
    )
    .await?;
    let select = match c.kind {
        DbKind::Mysql => format!("SELECT * FROM `{database}`.agentdb_smoke"),
        DbKind::Postgres | DbKind::Pgvector => {
            "SELECT * FROM agentdb_smoke".to_string()
        }
        DbKind::Sqlite => "SELECT * FROM agentdb_smoke".to_string(),
        _ => "SELECT * FROM agentdb_smoke".to_string(),
    };
    let page = s.query_page(&database, &select, 1, 10).await?;
    if page.rows.is_empty() {
        return Err("query returned 0 rows".into());
    }
    let cols = s.list_columns(&database, None, "agentdb_smoke").await?;
    if cols.is_empty() {
        return Err("no columns".into());
    }
    let _ = s.list_indexes(&database, None, "agentdb_smoke").await?;
    let _ = s.list_foreign_keys(&database, None, "agentdb_smoke").await?;
    let pk = json!({"id": page.rows[0][0].clone()})
        .as_object()
        .cloned()
        .unwrap_or_default();
    let upd = json!({"name": "smoke2"})
        .as_object()
        .cloned()
        .unwrap_or_default();
    s.update_row(&database, "agentdb_smoke", pk.clone(), upd)
        .await?;
    s.delete_row(&database, "agentdb_smoke", pk).await?;
    Ok(())
}

#[tokio::main]
async fn main() {
    let mut failed = 0;

    let mysql = conn("mysql", DbKind::Mysql, "localhost", 3307, Some("root"), Some("root"));
    if !check("mysql full", exercise_sql(&mysql, "")).await {
        failed += 1;
    }

    let mut pg = conn(
        "pg",
        DbKind::Postgres,
        "localhost",
        15432,
        Some("postgres"),
        Some("postgres"),
    );
    pg.database = Some("postgres".into());
    if !check("postgres full", exercise_sql(&pg, "postgres")).await {
        failed += 1;
    }

    let mut pgv = conn(
        "pgv",
        DbKind::Pgvector,
        "localhost",
        15433,
        Some("postgres"),
        Some("postgres"),
    );
    pgv.database = Some("postgres".into());
    if !check("pgvector full", exercise_sql(&pgv, "postgres")).await {
        failed += 1;
    }

    let mut lite = conn("lite", DbKind::Sqlite, "", 0, None, None);
    let mut opts = HashMap::new();
    opts.insert(
        "path".to_string(),
        std::env::temp_dir()
            .join("agentdb_smoke.db")
            .to_string_lossy()
            .to_string(),
    );
    lite.options = opts;
    if !check("sqlite full", exercise_sql(&lite, "")).await {
        failed += 1;
    }

    let redis = conn("redis", DbKind::Redis, "localhost", 16379, None, Some("123456"));
    if !check(
        "redis full",
        async {
            let s = db::open_session(&redis).await?;
            s.ping().await?;
            let _ = s.list_databases().await?;
            let _ = s.list_tables("db0", None).await?;
            let data = json!({"key": "agentdb:smoke", "value": "hello"})
                .as_object()
                .cloned()
                .unwrap_or_default();
            s.insert_row("db0", "string", data).await?;
            let page = s.query_page("db0", "agentdb:*", 1, 10).await?;
            if page.rows.is_empty() {
                return Err("redis key not found".into());
            }
            let pk = json!({"key": "agentdb:smoke"})
                .as_object()
                .cloned()
                .unwrap_or_default();
            s.delete_row("db0", "string", pk).await?;
            Ok(())
        },
    )
    .await
    {
        failed += 1;
    }

    let mut mongo = conn(
        "mongo",
        DbKind::Mongodb,
        "localhost",
        27017,
        Some("admin"),
        Some("tradingagents123"),
    );
    let mut mopts = HashMap::new();
    mopts.insert("authSource".to_string(), "admin".into());
    mongo.options = mopts;
    if !check(
        "mongo full",
        async {
            let s = db::open_session(&mongo).await?;
            s.ping().await?;
            let _ = s.list_databases().await?;
            let database = "admin".to_string();
            let _ = s.list_tables(&database, None).await?;
            let data = json!({"tag": "smoke", "n": 1})
                .as_object()
                .cloned()
                .unwrap_or_default();
            s.insert_row(&database, "agentdb_smoke", data).await?;
            let page = s.query_page(&database, "agentdb_smoke|{}", 1, 10).await?;
            if page.rows.is_empty() {
                return Err("mongo no docs".into());
            }
            let _ = s.list_columns(&database, None, "agentdb_smoke").await?;
            let _ = s.list_indexes(&database, None, "agentdb_smoke").await?;
            if let Some(id) = page.rows[0].first() {
                let pk = json!({"_id": id}).as_object().cloned().unwrap_or_default();
                s.delete_row(&database, "agentdb_smoke", pk).await?;
            }
            Ok(())
        },
    )
    .await
    {
        failed += 1;
    }

    let es = conn("es", DbKind::Elasticsearch, "localhost", 9200, None, None);
    if !check(
        "elasticsearch full",
        async {
            let s = db::open_session(&es).await?;
            s.ping().await?;
            let _ = s.list_databases().await?;
            let _ = s.list_tables("", None).await?;
            let data = json!({"title": "smoke", "n": 1})
                .as_object()
                .cloned()
                .unwrap_or_default();
            s.insert_row("", "agentdb_smoke", data).await?;
            let page = s
                .query_page(
                    "",
                    "agentdb_smoke|{\"query\":{\"match_all\":{}}}",
                    1,
                    10,
                )
                .await?;
            if page.rows.is_empty() {
                return Err("es no docs".into());
            }
            let _ = s.list_columns("", None, "agentdb_smoke").await?;
            if let Some(id) = page
                .columns
                .iter()
                .position(|c| c == "_id")
                .and_then(|i| page.rows[0].get(i).cloned())
            {
                let pk = json!({"_id": id}).as_object().cloned().unwrap_or_default();
                s.delete_row("", "agentdb_smoke", pk).await?;
            }
            Ok(())
        },
    )
    .await
    {
        failed += 1;
    }

    let qdrant = conn("qdrant", DbKind::Qdrant, "localhost", 6333, None, None);
    if check(
        "qdrant ping",
        async {
            let s = db::open_session(&qdrant).await?;
            s.ping().await?;
            Ok(())
        },
    )
    .await
    {
        if !check(
            "qdrant list",
            async {
                let s = db::open_session(&qdrant).await?;
                let _ = s.list_tables("default", None).await?;
                Ok(())
            },
        )
        .await
        {
            failed += 1;
        }
    } else {
        println!("SKIP  qdrant (service not running)");
    }

    if !check("models serde", async {
        let s = AiSettings::default();
        let t = serde_json::to_string(&s).map_err(|e| e.to_string())?;
        let _: AiSettings = serde_json::from_str(&t).map_err(|e| e.to_string())?;
        Ok(())
    })
    .await
    {
        failed += 1;
    }

    if failed > 0 {
        eprintln!("{failed} checks failed");
        std::process::exit(1);
    }
    println!("ALL CHECKS PASSED");
}
