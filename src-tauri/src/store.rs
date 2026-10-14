use crate::models::{AiSettings, ChatSession, ConnectionConfig, MemoryStore};
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
    /// 配置全部放在 `~/.db_tools/` 这一个目录里（同多数 CLI/桌面工具的点目录约定），
    /// 代码里不内置任何连接信息：没有 connections.json 就是空列表，界面引导新建。
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

    /// 智能体图片目录：会话 JSON 里只存文件名，base64 不进库
    pub fn ai_images_dir(&self) -> PathBuf {
        let d = self.data_dir.join("ai_images");
        let _ = std::fs::create_dir_all(&d);
        d
    }

    /// 前端传来的图片名先校验再拼路径，`../` 之类走不出去
    fn image_path(&self, name: &str) -> Result<PathBuf, String> {
        let ok = !name.is_empty()
            && name.len() <= 80
            && name.chars().all(|c| {
                c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.'
            });
        if !ok {
            return Err(format!("非法图片名: {name}"));
        }
        Ok(self.ai_images_dir().join(name))
    }

    pub fn save_ai_image(&self, name: &str, bytes: &[u8]) -> Result<(), String> {
        let path = self.image_path(name)?;
        std::fs::write(&path, bytes).map_err(|e| format!("写入图片失败: {e}"))
    }

    pub fn read_ai_image(&self, name: &str) -> Result<Vec<u8>, String> {
        let path = self.image_path(name)?;
        std::fs::read(&path).map_err(|e| format!("读取图片失败: {e}"))
    }

    pub fn remove_ai_images(&self, names: &[String]) {
        for n in names {
            if let Ok(path) = self.image_path(n) {
                let _ = std::fs::remove_file(path);
            }
        }
    }
}
