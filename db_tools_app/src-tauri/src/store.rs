use crate::models::{AiSettings, ChatSession, ConnectionConfig, MemoryStore};
use chrono::Utc;
use uuid::Uuid;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};

/// Recover from lock poisoning instead of panicking the whole app.
pub fn lock_or_recover<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// 读模型配置：新版是数组；旧版单对象升成一条默认配置，migrated=true 时要回写
pub fn parse_ai_profiles(text: &str) -> (Vec<AiSettings>, bool) {
    if let Ok(list) = serde_json::from_str::<Vec<AiSettings>>(text) {
        return (list, false);
    }
    if let Ok(mut one) = serde_json::from_str::<AiSettings>(text) {
        if one.id.is_empty() {
            one.id = Uuid::new_v4().to_string();
        }
        one.is_default = true;
        return (vec![one], true);
    }
    (Vec::new(), false)
}

/// 列表不变式：至少一条，且恰好一条默认
pub fn normalize_profiles(mut list: Vec<AiSettings>) -> Vec<AiSettings> {
    if list.is_empty() {
        list.push(AiSettings {
            id: Uuid::new_v4().to_string(),
            ..Default::default()
        });
    }
    let mut seen = false;
    for p in list.iter_mut() {
        if !p.is_default {
            continue;
        }
        if seen {
            p.is_default = false;
        }
        seen = true;
    }
    if !seen {
        list[0].is_default = true;
    }
    list
}

/// id 为空即新增；同 id 覆盖
pub fn upsert_profile(list: &[AiSettings], profile: AiSettings) -> Vec<AiSettings> {
    let mut out = list.to_vec();
    match out.iter_mut().find(|p| p.id == profile.id) {
        Some(slot) => *slot = profile,
        None => out.push(profile),
    }
    normalize_profiles(out)
}

pub fn remove_profile(list: &[AiSettings], id: &str) -> Vec<AiSettings> {
    normalize_profiles(list.iter().filter(|p| p.id != id).cloned().collect())
}

pub fn mark_default(list: &[AiSettings], id: &str) -> Vec<AiSettings> {
    let mut out = list.to_vec();
    for p in out.iter_mut() {
        p.is_default = p.id == id;
    }
    normalize_profiles(out)
}

/// 智能体取配置：显式指定 > 默认 > 第一条
pub fn resolve_profile(list: &[AiSettings], id: Option<&str>) -> Option<AiSettings> {
    let want = id.map(|s| s.trim()).filter(|s| !s.is_empty());
    match want {
        Some(w) => list
            .iter()
            .find(|p| p.id == w)
            .or_else(|| list.iter().find(|p| p.is_default))
            .or_else(|| list.first())
            .cloned(),
        None => list
            .iter()
            .find(|p| p.is_default)
            .or_else(|| list.first())
            .cloned(),
    }
}

pub struct AppState {
    pub connections: Mutex<Vec<ConnectionConfig>>,
    pub sessions: Mutex<Vec<ChatSession>>,
    pub memory: Mutex<MemoryStore>,
    pub ai_profiles: Mutex<Vec<AiSettings>>,
    pub data_dir: PathBuf,
}

impl AppState {
    pub fn new() -> Self {
        let data_dir = dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".db_tools");
        let _ = std::fs::create_dir_all(&data_dir);
        let state = Self {
            connections: Mutex::new(Vec::new()),
            sessions: Mutex::new(Vec::new()),
            memory: Mutex::new(MemoryStore::default()),
            ai_profiles: Mutex::new(Vec::new()),
            data_dir,
        };
        state.load_all();
        if lock_or_recover(&state.connections).is_empty() {
            state.seed_defaults();
        }
        state
    }

    fn paths(&self) -> (PathBuf, PathBuf, PathBuf, PathBuf) {
        (
            self.data_dir.join("connections.json"),
            self.data_dir.join("chat_sessions.json"),
            self.data_dir.join("memory.json"),
            self.data_dir.join("ai_settings.json"),
        )
    }

    fn load_all(&self) {
        let (c, s, m, a) = self.paths();
        if let Ok(text) = std::fs::read_to_string(&c) {
            if let Ok(v) = serde_json::from_str::<Vec<ConnectionConfig>>(&text) {
                *lock_or_recover(&self.connections) = v;
            }
        }
        if let Ok(text) = std::fs::read_to_string(&s) {
            if let Ok(v) = serde_json::from_str::<Vec<ChatSession>>(&text) {
                *lock_or_recover(&self.sessions) = v;
            }
        }
        if let Ok(text) = std::fs::read_to_string(&m) {
            if let Ok(v) = serde_json::from_str::<MemoryStore>(&text) {
                *lock_or_recover(&self.memory) = v;
            }
        }
        if let Ok(text) = std::fs::read_to_string(&a) {
            let (list, migrated) = parse_ai_profiles(&text);
            *lock_or_recover(&self.ai_profiles) = list;
            if migrated {
                self.save_ai_profiles();
            }
        }
        let fixed = normalize_profiles(lock_or_recover(&self.ai_profiles).clone());
        *lock_or_recover(&self.ai_profiles) = fixed;
    }

    pub fn save_connections(&self) {
        let (c, _, _, _) = self.paths();
        let data = lock_or_recover(&self.connections).clone();
        let _ = std::fs::write(c, serde_json::to_string_pretty(&data).unwrap_or_default());
    }

    pub fn save_sessions(&self) {
        let (_, s, _, _) = self.paths();
        let data = lock_or_recover(&self.sessions).clone();
        let _ = std::fs::write(s, serde_json::to_string_pretty(&data).unwrap_or_default());
    }

    pub fn save_memory(&self) {
        let (_, _, m, _) = self.paths();
        let data = lock_or_recover(&self.memory).clone();
        let _ = std::fs::write(m, serde_json::to_string_pretty(&data).unwrap_or_default());
    }

    pub fn save_ai_profiles(&self) {
        let (_, _, _, a) = self.paths();
        let data = lock_or_recover(&self.ai_profiles).clone();
        let _ = std::fs::write(a, serde_json::to_string_pretty(&data).unwrap_or_default());
    }

    fn seed_defaults(&self) {
        let now = Utc::now().to_rfc3339();
        let defaults = vec![
            ConnectionConfig {
                id: "local-mysql".into(),
                name: "本地 MySQL".into(),
                kind: crate::models::DbKind::Mysql,
                host: Some("localhost".into()),
                port: Some(3307),
                username: Some("root".into()),
                password: Some("root".into()),
                database: None,
                options: Default::default(),
                color: Some("#00758F".into()),
                created_at: Some(now.clone()),
                updated_at: Some(now.clone()),
            },
            ConnectionConfig {
                id: "local-postgres".into(),
                name: "本地 PostgreSQL".into(),
                kind: crate::models::DbKind::Postgres,
                host: Some("localhost".into()),
                port: Some(15432),
                username: Some("postgres".into()),
                password: Some("postgres".into()),
                database: Some("postgres".into()),
                options: Default::default(),
                color: Some("#336791".into()),
                created_at: Some(now.clone()),
                updated_at: Some(now.clone()),
            },
            ConnectionConfig {
                id: "local-pgvector".into(),
                name: "本地 PgVector".into(),
                kind: crate::models::DbKind::Pgvector,
                host: Some("localhost".into()),
                port: Some(15433),
                username: Some("postgres".into()),
                password: Some("postgres".into()),
                database: Some("postgres".into()),
                options: Default::default(),
                color: Some("#4169E1".into()),
                created_at: Some(now.clone()),
                updated_at: Some(now.clone()),
            },
            ConnectionConfig {
                id: "local-redis".into(),
                name: "本地 Redis".into(),
                kind: crate::models::DbKind::Redis,
                host: Some("localhost".into()),
                port: Some(16379),
                username: None,
                password: Some("123456".into()),
                database: Some("0".into()),
                options: Default::default(),
                color: Some("#DC382D".into()),
                created_at: Some(now.clone()),
                updated_at: Some(now.clone()),
            },
            ConnectionConfig {
                id: "local-mongodb".into(),
                name: "本地 MongoDB".into(),
                kind: crate::models::DbKind::Mongodb,
                host: Some("localhost".into()),
                port: Some(27017),
                username: Some("admin".into()),
                password: Some("tradingagents123".into()),
                database: None,
                options: {
                    let mut m = std::collections::HashMap::new();
                    m.insert("authSource".to_string(), "admin".to_string());
                    m
                },
                color: Some("#47A248".into()),
                created_at: Some(now.clone()),
                updated_at: Some(now.clone()),
            },
            ConnectionConfig {
                id: "local-elastic".into(),
                name: "本地 Elasticsearch".into(),
                kind: crate::models::DbKind::Elasticsearch,
                host: Some("localhost".into()),
                port: Some(9200),
                username: None,
                password: None,
                database: None,
                options: Default::default(),
                color: Some("#00BFB3".into()),
                created_at: Some(now.clone()),
                updated_at: Some(now.clone()),
            },
            ConnectionConfig {
                id: "local-sqlite".into(),
                name: "本地 SQLite".into(),
                kind: crate::models::DbKind::Sqlite,
                host: None,
                port: None,
                username: None,
                password: None,
                database: None,
                options: {
                    let mut m = std::collections::HashMap::new();
                    let path = self.data_dir.join("local.db").to_string_lossy().to_string();
                    m.insert("path".to_string(), path);
                    m
                },
                color: Some("#003B57".into()),
                created_at: Some(now.clone()),
                updated_at: Some(now.clone()),
            },
            ConnectionConfig {
                id: "local-qdrant".into(),
                name: "本地 Qdrant".into(),
                kind: crate::models::DbKind::Qdrant,
                host: Some("localhost".into()),
                port: Some(6333),
                username: None,
                password: None,
                database: None,
                options: Default::default(),
                color: Some("#DC244C".into()),
                created_at: Some(now.clone()),
                updated_at: Some(now.clone()),
            },
        ];
        *lock_or_recover(&self.connections) = defaults;
        self.save_connections();
    }
}
