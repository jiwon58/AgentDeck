use crate::model::now;
use serde_json::json;
use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        OnceLock,
    },
};
use uuid::Uuid;

const MAX_BYTES: u64 = 1024 * 1024;
static LOG: OnceLock<Log> = OnceLock::new();

struct Log {
    path: PathBuf,
    run_id: Uuid,
    available: AtomicBool,
}

impl Log {
    fn open(directory: &Path) -> io::Result<Self> {
        fs::create_dir_all(directory)?;
        let path = directory.join("agent-deck.log");
        OpenOptions::new().create(true).append(true).open(&path)?;
        Ok(Self {
            path,
            run_id: Uuid::new_v4(),
            available: AtomicBool::new(true),
        })
    }

    fn append(&self, line: &[u8], limit: u64) -> io::Result<()> {
        // Keep the lock separate from the log that is renamed during rotation.
        // Closing this handle releases the lock, including on error.
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(self.path.with_extension("lock"))?;
        lock.lock()?;
        let size = match fs::metadata(&self.path) {
            Ok(metadata) => metadata.len(),
            Err(err) if err.kind() == io::ErrorKind::NotFound => 0,
            Err(err) => return Err(err),
        };
        if size + line.len() as u64 > limit {
            let previous = self.path.with_extension("log.1");
            match fs::remove_file(&previous) {
                Ok(()) => {}
                Err(err) if err.kind() == io::ErrorKind::NotFound => {}
                Err(err) => return Err(err),
            }
            fs::rename(&self.path, previous)?;
        }
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?
            .write_all(line)
    }
}

pub fn init(directory: Option<&Path>) {
    if LOG.get().is_some() {
        return;
    }
    let preferred = directory.and_then(|path| Log::open(path).ok());
    if let Some(log) = preferred {
        let _ = LOG.set(log);
    } else if let Ok(log) = Log::open(&std::env::temp_dir().join("AgentDeck").join("logs")) {
        let _ = LOG.set(log);
        warn("app.log_fallback", None);
    }
}

pub fn path() -> Option<String> {
    LOG.get()
        .filter(|log| log.available.load(Ordering::Relaxed))
        .map(|log| log.path.to_string_lossy().into_owned())
}

// Only static event/detail names, UUIDs and numbers can reach the file.
// Never forward Debug/Display errors or subscribe to dependency logs here.
pub fn record(
    level: &'static str,
    event: &'static str,
    entity: Option<&str>,
    code: Option<i64>,
    outcome: Option<&'static str>,
) {
    let Some(log) = LOG.get() else {
        return;
    };
    let entity_id = entity.and_then(|id| Uuid::parse_str(id).ok());
    let line = json!({
        "time_unix_ms": now(), "level": level, "event": event, "run_id": log.run_id,
        "pid": std::process::id(), "entity_id": entity_id, "code": code, "outcome": outcome,
    });
    let result = log.append(format!("{line}\n").as_bytes(), MAX_BYTES);
    log.available.store(result.is_ok(), Ordering::Relaxed);
}

pub fn info(event: &'static str, entity: Option<&str>) {
    record("INFO", event, entity, None, None);
}
pub fn warn(event: &'static str, entity: Option<&str>) {
    record("WARN", event, entity, None, None);
}

pub fn stage<T, E>(
    result: std::result::Result<T, E>,
    event: &'static str,
    entity: Option<&str>,
) -> std::result::Result<T, E> {
    result.inspect_err(|_| warn(event, entity))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotates_and_keeps_only_one_previous_file() {
        let directory = tempfile::tempdir().expect("temporary logs");
        let log = Log::open(directory.path()).expect("create logger");
        for line in [b"first!\n", b"second\n", b"third!\n"] {
            log.append(line, 8).expect("append and rotate");
        }
        assert_eq!(fs::read(&log.path).expect("active log"), b"third!\n");
        assert_eq!(
            fs::read(log.path.with_extension("log.1")).expect("previous log"),
            b"second\n"
        );
        assert_eq!(
            fs::read_dir(directory.path()).expect("list logs").count(),
            3
        );
    }
}
