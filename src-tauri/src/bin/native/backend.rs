use agent_deck::{
    gjc_history::{self, Source},
    model::{self, Agent, Result, Router, Runtime, Session},
    native_gjc::{self, Preset, TOKEN},
    registry,
    storage::{Store, Table},
    terminal::{SessionResources, Terminal},
};
use portable_pty::CommandBuilder;
use std::{path::PathBuf, sync::Mutex};

pub struct Backend {
    pub data: PathBuf,
    pub store: Store,
    pub terminal: Terminal,
    launching: Mutex<()>,
}

pub enum Mode {
    Direct {
        cwd: String,
    },
    Preset {
        cwd: String,
        preset: Box<Preset>,
        token: String,
        catalog: Vec<String>,
    },
    Resume {
        source: Source,
        model: String,
        token: String,
    },
}

impl Backend {
    pub fn open(data: PathBuf) -> Result<Self> {
        std::fs::create_dir_all(&data).map_err(model::error)?;
        let store = Store::open(&data.join("deck.db"))?;
        Ok(Self {
            data,
            store,
            terminal: Terminal::default(),
            launching: Mutex::new(()),
        })
    }

    pub fn register(&self, path: &str, digest: &str, agent: Agent) -> Result<Runtime> {
        let candidate = registry::inspect(path)?;
        if candidate.sha256 != digest {
            return Err("Executable changed after inspection. Review it again.".into());
        }
        let version = registry::version(&candidate, &[])?;
        let runtime = Runtime {
            id: model::id(),
            agent,
            name: std::path::Path::new(path)
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or(path)
                .into(),
            version: version.clone().unwrap_or_else(|| "Unverified".into()),
            executable: candidate,
            arguments: vec![],
            root: String::new(),
            cwd_override: None,
            dependencies: String::new(),
            router_environment: Default::default(),
            trusted: true,
            verified: version.is_some(),
            last_verified_at: version.map(|_| model::now()),
            created_at: model::now(),
        };
        self.store.put(Table::Runtimes, &runtime.id, &runtime)?;
        Ok(runtime)
    }

    pub fn launch(
        &self,
        name: String,
        runtime_id: &str,
        mode: Mode,
        wake: impl Fn() + Send + Sync + 'static,
    ) -> Result<Session> {
        let _launch = self.launching.lock().map_err(model::error)?;
        let id = uuid::Uuid::new_v4();
        let runtime: Runtime = self
            .store
            .get(Table::Runtimes, runtime_id)?
            .ok_or("Select a registered runtime")?;
        if !runtime.trusted {
            return Err("Approve the executable first".into());
        }
        let executable = registry::verify(&runtime.executable)?;
        let mut command = CommandBuilder::new(&executable.path);
        let mut resources = SessionResources::default();
        let mut session = Session {
            id: id.to_string(),
            name,
            runtime_id: runtime.id,
            agent: runtime.agent,
            version: runtime.version,
            executable,
            cwd: String::new(),
            router_id: None,
            endpoint: None,
            router_revision: None,
            model: String::new(),
            created_at: model::now(),
            resume_source_id: None,
            gjc: None,
            team: false,
            external_history: None,
            native_gjc: None,
        };
        let cwd = match mode {
            Mode::Direct { cwd } => {
                command.args(runtime.arguments);
                registry::directory(runtime.cwd_override.as_deref().unwrap_or(&cwd))?
            }
            Mode::Preset {
                cwd,
                preset,
                token,
                catalog,
            } => {
                require_gjc(&session, runtime.verified)?;
                let cwd = registry::directory(&cwd)?;
                let profile: Router = self
                    .store
                    .get(Table::Routers, &preset.router_id)?
                    .ok_or("Select a Router profile")?;
                let documents = native_gjc::refresh_documents(&preset, &profile, &token, &catalog)?;
                let paths = native_gjc::prepare(&self.data, &cwd, id, &preset, &documents)?;
                let token = if profile.authentication == "bearer" {
                    token.as_str()
                } else {
                    ""
                };
                native_gjc::configure_command(
                    &mut command,
                    &paths,
                    &session.version,
                    token,
                    Some(&documents.endpoint),
                );
                command.arg("--session-dir");
                command.arg(&paths.session_directory);
                command.args(["--model", &documents.selector]);
                session.model = documents.selector;
                session.endpoint = Some(documents.endpoint);
                session.router_revision = Some(profile.updated_at);
                session.router_id = Some(profile.id);
                session.native_gjc = Some(paths);
                cwd
            }
            Mode::Resume {
                source,
                model,
                token,
            } => {
                require_gjc(&session, runtime.verified)?;
                let agent = gjc_history::agent_directory(source.agent_directory.to_str())?;
                let entry = gjc_history::inspect(&source.path, agent)?;
                if entry.source.session_id != source.session_id {
                    return Err("The GJC session file changed. Select it again.".into());
                }
                resources.history_lease = Some(gjc_history::lease(&self.data, &entry.source)?);
                let previous = self
                    .store
                    .list::<Session>(Table::Sessions)?
                    .into_iter()
                    .find(|saved| {
                        saved
                            .external_history
                            .as_ref()
                            .is_some_and(|history| history.same_file(&entry.source))
                            || saved.native_gjc.as_ref().is_some_and(|paths| {
                                paths.agent_directory == entry.source.agent_directory
                                    && entry.source.path.parent()
                                        == Some(paths.session_directory.as_path())
                            })
                    });
                if let Some(saved) = previous {
                    if saved.external_history.as_ref().is_some_and(|history| {
                        history.session_id != entry.source.session_id
                            || history.agent_directory != entry.source.agent_directory
                    }) {
                        return Err("The saved conversation or its agent directory changed".into());
                    }
                    if self.terminal.history_active(&saved.id)? {
                        return Err(
                            "Close the other Deck tab before resuming this conversation".into()
                        );
                    }
                    if saved.native_gjc.is_some() && session.version == "0.20.0" {
                        command.env("GJC_MODEL_PRESET_REGISTRY_DISABLED", "1");
                    }
                    session.resume_source_id = Some(saved.id);
                }
                if !token.is_empty() {
                    command.env(TOKEN, token);
                }
                command.env("GJC_CODING_AGENT_DIR", &entry.source.agent_directory);
                command.arg("--resume");
                command.arg(&entry.source.path);
                if !model.is_empty() {
                    if model.starts_with('-') || model.contains('\0') {
                        return Err("Enter a valid GJC model selector".into());
                    }
                    command.args(["--model", model.as_str()]);
                }
                session.external_history = Some(entry.source);
                registry::directory(entry.cwd.to_str().ok_or("Invalid GJC working directory")?)?
            }
        };
        if session.agent == Agent::Gajae {
            command.env("GJC_HOST_STATUS", "terax");
        }
        command.cwd(&cwd);
        session.cwd = cwd.to_string_lossy().into_owned();
        self.terminal.start_with_resources(
            session.id.clone(),
            session.resume_source_id.as_deref(),
            command,
            (100, 30),
            move |_| wake(),
            resources,
        )?;
        if session.resume_source_id.is_none() {
            if let Err(error) = self.store.put(Table::Sessions, &session.id, &session) {
                let _ = self.terminal.close(&session.id);
                return Err(error);
            }
        }
        Ok(session)
    }
}

fn require_gjc(session: &Session, verified: bool) -> Result<()> {
    if session.agent == Agent::Gajae
        && verified
        && matches!(session.version.as_str(), "0.10.2" | "0.20.0")
    {
        Ok(())
    } else {
        Err("Choose a verified GJC 0.10.2 or 0.20.0 runtime".into())
    }
}

#[cfg(test)]
#[path = "backend_tests.rs"]
mod tests;
