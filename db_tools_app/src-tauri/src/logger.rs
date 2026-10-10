use chrono::Utc;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

static LOG_FILE: Mutex<Option<PathBuf>> = Mutex::new(None);

pub fn init(data_dir: &PathBuf) {
    let _ = std::fs::create_dir_all(data_dir.join("logs"));
    let path = data_dir.join("logs").join("app.log");
    // rotate if > 5MB
    if let Ok(meta) = std::fs::metadata(&path) {
        if meta.len() > 5 * 1024 * 1024 {
            let bak = data_dir.join("logs").join("app.old.log");
            let _ = std::fs::rename(&path, bak);
        }
    }
    if let Ok(mut g) = LOG_FILE.lock() {
        *g = Some(path);
    }
}

fn path() -> Option<PathBuf> {
    LOG_FILE.lock().ok().and_then(|g| g.clone())
}

fn write_line(level: &str, target: &str, message: &str) {
    let line = format!(
        "{} [{}] [{}] {}\n",
        Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ"),
        level,
        target,
        message.replace('\n', " | ")
    );
    if let Some(p) = path() {
        if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&p) {
            let _ = f.write_all(line.as_bytes());
        }
    }
    // also mirror to stderr for tauri_dev.log
    eprint!("{line}");
}

pub fn info(target: &str, message: impl AsRef<str>) {
    write_line("INFO", target, message.as_ref());
}

pub fn warn(target: &str, message: impl AsRef<str>) {
    write_line("WARN", target, message.as_ref());
}

pub fn error(target: &str, message: impl AsRef<str>) {
    write_line("ERROR", target, message.as_ref());
}

/// Install panic hook so crashes are captured before abort/unwind.
pub fn install_panic_hook() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let payload = info.payload();
        let msg = if let Some(s) = payload.downcast_ref::<&str>() {
            (*s).to_string()
        } else if let Some(s) = payload.downcast_ref::<String>() {
            s.clone()
        } else {
            "unknown panic".to_string()
        };
        let loc = info
            .location()
            .map(|l| format!("{}:{}", l.file(), l.line()))
            .unwrap_or_else(|| "unknown".into());
        error("panic", format!("{msg} @ {loc}"));
        default(info);
    }));
}

/// Read recent log tail for diagnostics.
pub fn tail(max_lines: usize) -> String {
    let Some(p) = path() else {
        return String::new();
    };
    let Ok(text) = std::fs::read_to_string(&p) else {
        return String::new();
    };
    let lines: Vec<&str> = text.lines().collect();
    let start = lines.len().saturating_sub(max_lines);
    lines[start..].join("\n")
}

/// Frontend-reported errors.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct FrontendError {
    pub message: String,
    pub stack: Option<String>,
    pub source: Option<String>,
}

pub fn log_frontend(err: FrontendError) {
    error(
        "frontend",
        format!(
            "{} | stack={:?} | source={:?}",
            err.message, err.stack, err.source
        ),
    );
}

// silence unused File import warning in some rustc versions
#[allow(dead_code)]
fn _unused(_: Option<File>) {}
