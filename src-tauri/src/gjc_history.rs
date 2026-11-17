use crate::{
    gjc,
    model::{error, Result},
    registry,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
};

const HEADER_LIMIT: u64 = 64 * 1024;
const SCAN_LIMIT: usize = 5000;

#[derive(Clone, Serialize, Deserialize)]
pub struct Source {
    pub path: PathBuf,
    pub agent_directory: PathBuf,
    pub session_id: String,
}

impl Source {
    pub fn same_file(&self, other: &Self) -> bool {
        self.path == other.path
            || (cfg!(windows)
                && self.path.to_string_lossy().to_lowercase()
                    == other.path.to_string_lossy().to_lowercase())
    }
}

#[derive(Serialize)]
pub struct Entry {
    pub source: Source,
    pub cwd: PathBuf,
    pub title: String,
    pub modified_at: u64,
}

#[derive(Deserialize)]
struct Header {
    #[serde(rename = "type")]
    kind: String,
    id: String,
    cwd: PathBuf,
    title: Option<String>,
}

pub fn agent_directory(path: Option<&str>) -> Result<PathBuf> {
    match path.filter(|path| !path.is_empty()) {
        Some(path) => registry::directory(path),
        None => {
            let default = gjc::default_agent_directory()?;
            registry::directory(default.to_str().ok_or("Invalid GJC agent directory")?)
        }
    }
}

pub fn lease(app_data: &Path, source: &Source) -> Result<File> {
    let root = app_data.join("gjc/history-locks");
    fs::create_dir_all(&root).map_err(error)?;
    let identity = source.path.to_str().ok_or("Invalid GJC history filename")?;
    let identity = if cfg!(windows) {
        identity.to_lowercase()
    } else {
        identity.to_owned()
    };
    let digest = Sha256::digest(identity.as_bytes());
    let file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(root.join(format!("{digest:x}.lock")))
        .map_err(error)?;
    file.try_lock().map_err(|failure| match failure {
        std::fs::TryLockError::WouldBlock => {
            "Close the other Deck tab before resuming this conversation".into()
        }
        std::fs::TryLockError::Error(error) => error.to_string(),
    })?;
    Ok(file)
}

pub fn inspect(path: &Path, agent_directory: PathBuf) -> Result<Entry> {
    if !path.is_absolute()
        || !path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("jsonl"))
    {
        return Err("Select an absolute path to a GJC .jsonl session file".into());
    }
    let path = dunce::canonicalize(path).map_err(error)?;
    let metadata = fs::metadata(&path).map_err(error)?;
    if !metadata.is_file() {
        return Err("The GJC history path must be a file".into());
    }
    let mut reader = BufReader::new(File::open(&path).map_err(error)?).take(HEADER_LIMIT);
    let mut line = String::new();
    reader.read_line(&mut line).map_err(error)?;
    if !line.ends_with('\n') {
        return Err("Incomplete or oversized GJC session header".into());
    }
    let header: Header = serde_json::from_str(line.trim_start_matches('\u{feff}'))
        .map_err(|_| "Invalid GJC session header")?;
    if header.kind != "session"
        || header.id.is_empty()
        || header.id.len() > 128
        || !header.cwd.is_absolute()
    {
        return Err(
            "The file does not identify a GJC session and its original working directory".into(),
        );
    }
    Ok(Entry {
        source: Source {
            path,
            agent_directory,
            session_id: header.id,
        },
        cwd: header.cwd,
        title: header.title.unwrap_or_default().chars().take(160).collect(),
        modified_at: metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |time| time.as_millis() as u64),
    })
}

#[derive(Serialize)]
pub struct Catalog {
    pub entries: Vec<Entry>,
    pub limited: bool,
}

pub fn list(agent: PathBuf) -> Result<Catalog> {
    list_in(agent.join("sessions"), agent)
}

pub fn list_in(root: PathBuf, agent: PathBuf) -> Result<Catalog> {
    if !root.is_dir() {
        return Ok(Catalog {
            entries: vec![],
            limited: false,
        });
    }
    let mut paths = vec![(root, 0)];
    let mut entries = vec![];
    let mut visited = 0;
    while let Some((folder, depth)) = paths.pop() {
        for item in fs::read_dir(folder).map_err(error)?.flatten() {
            visited += 1;
            if visited > SCAN_LIMIT {
                break;
            }
            let Ok(kind) = item.file_type() else { continue };
            if kind.is_dir() && depth < 2 {
                paths.push((item.path(), depth + 1));
            } else if kind.is_file() {
                if let Ok(entry) = inspect(&item.path(), agent.clone()) {
                    entries.push(entry);
                }
            }
        }
        if visited > SCAN_LIMIT {
            break;
        }
    }
    entries.sort_unstable_by_key(|entry| std::cmp::Reverse(entry.modified_at));
    let limited = visited > SCAN_LIMIT || entries.len() > 200;
    entries.truncate(200);
    Ok(Catalog { entries, limited })
}
