use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    time::{SystemTime, UNIX_EPOCH},
};

pub type Result<T> = std::result::Result<T, String>;
pub fn id() -> String {
    uuid::Uuid::new_v4().to_string()
}
pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
pub fn error(e: impl std::fmt::Display) -> String {
    e.to_string()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Agent {
    Gajae,
    Claude,
    Hermes,
    Generic,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Root {
    pub id: String,
    pub agent: Agent,
    pub path: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Candidate {
    pub path: String,
    pub sha256: String,
    pub architecture: String,
    pub launch_type: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Runtime {
    pub id: String,
    pub agent: Agent,
    pub name: String,
    pub version: String,
    pub executable: Candidate,
    pub arguments: Vec<String>,
    pub root: String,
    pub cwd_override: Option<String>,
    pub dependencies: String,
    #[serde(default)]
    pub router_environment: BTreeMap<String, String>,
    pub trusted: bool,
    pub verified: bool,
    pub last_verified_at: Option<u64>,
    pub created_at: u64,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Router {
    pub id: String,
    pub name: String,
    pub scheme: String,
    pub host: String,
    pub port: u16,
    pub openai_path: Option<String>,
    pub anthropic_path: Option<String>,
    pub model: String,
    pub authentication: String,
    pub enabled: bool,
    pub allow_remote: bool,
    pub created_at: u64,
    pub updated_at: u64,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub name: String,
    pub runtime_id: String,
    pub agent: Agent,
    pub version: String,
    pub executable: Candidate,
    pub cwd: String,
    pub router_id: Option<String>,
    pub endpoint: Option<String>,
    pub router_revision: Option<u64>,
    pub model: String,
    pub created_at: u64,
    #[serde(default)]
    pub resume_source_id: Option<String>,
}

#[derive(Deserialize)]
pub struct Launch {
    pub id: String,
    pub resume_from: Option<String>,
    pub name: String,
    pub runtime_id: String,
    pub cwd: String,
    pub router_id: Option<String>,
    pub model: String,
    pub extra: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub token: String,
    pub manual_router: bool,
    #[serde(default)]
    pub manual_protocol: String,
    pub cols: u16,
    pub rows: u16,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Preferences {
    pub font_size: u16,
    pub scrollback: u32,
    pub last_runtime: String,
    pub last_cwd: String,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            font_size: 14,
            scrollback: 3000,
            last_runtime: String::new(),
            last_cwd: String::new(),
        }
    }
}

#[derive(Serialize)]
pub struct Overview {
    pub app_version: &'static str,
    pub roots: Vec<Root>,
    pub runtimes: Vec<Runtime>,
    pub routers: Vec<Router>,
    pub sessions: Vec<Session>,
    pub preferences: Preferences,
    pub data_directory: String,
    pub log_file: Option<String>,
}
