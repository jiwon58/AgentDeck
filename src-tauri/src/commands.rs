use agent_deck::{
    adapter, diagnostics,
    model::{
        error, id, now, Agent, Candidate, Launch, Overview, Preferences, Result, Root, Router,
        Runtime, Session,
    },
    registry, router,
    storage::{Store, Table},
    terminal::{Event, Terminal},
};
use portable_pty::CommandBuilder;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use tauri::{AppHandle, Emitter, Manager, State, WebviewWindow};

pub struct Deck {
    pub store: Store,
    pub terminal: Arc<Terminal>,
    pub directory: String,
    pub allow_close: AtomicBool,
}

fn prefs(deck: &Deck) -> Result<Preferences> {
    Ok(deck
        .store
        .get(Table::Settings, "terminal")?
        .unwrap_or_default())
}
fn required<T: serde::de::DeserializeOwned>(
    deck: &Deck,
    table: Table,
    id: &str,
    message: &str,
) -> Result<T> {
    deck.store.get(table, id)?.ok_or_else(|| message.to_owned())
}

async fn blocking<T: Send + 'static>(
    event: &'static str,
    f: impl FnOnce() -> Result<T> + Send + 'static,
) -> Result<T> {
    diagnostics::stage(
        tokio::task::spawn_blocking(f).await.map_err(error)?,
        event,
        None,
    )
}

#[tauri::command]
pub fn overview(deck: State<'_, Deck>) -> Result<Overview> {
    Ok(Overview {
        app_version: env!("CARGO_PKG_VERSION"),
        roots: deck.store.list(Table::Roots)?,
        runtimes: deck.store.list(Table::Runtimes)?,
        routers: deck.store.list(Table::Routers)?,
        sessions: deck.store.list(Table::Sessions)?,
        preferences: prefs(&deck)?,
        data_directory: deck.directory.clone(),
        log_file: diagnostics::path(),
    })
}

#[tauri::command]
pub async fn scan(app: AppHandle, agent: Agent, path: String) -> Result<registry::Scan> {
    blocking("registry.scan_failed", move || {
        let path = registry::directory(&path)?.to_string_lossy().into_owned();
        let result = registry::scan(agent, &path)?;
        diagnostics::record(
            if result.warnings.is_empty() {
                "INFO"
            } else {
                "WARN"
            },
            "registry.scan_finished",
            None,
            Some(result.candidates.len() as i64),
            (!result.warnings.is_empty()).then_some("has_warnings"),
        );
        let deck = app.state::<Deck>();
        if !deck
            .store
            .list::<Root>(Table::Roots)?
            .iter()
            .any(|root| root.agent == agent && root.path.eq_ignore_ascii_case(&path))
        {
            let root = Root {
                id: id(),
                agent,
                path,
            };
            deck.store.put(Table::Roots, &root.id, &root)?;
        }
        Ok(result)
    })
    .await
}

#[tauri::command]
pub fn delete_root(deck: State<'_, Deck>, id: String) -> Result<()> {
    deck.store.delete(Table::Roots, &id)
}

#[tauri::command]
pub async fn inspect_executable(path: String) -> Result<Candidate> {
    blocking("registry.inspect_failed", move || registry::inspect(&path)).await
}

#[derive(serde::Deserialize)]
pub struct Registration {
    agent: Agent,
    name: String,
    candidate: Candidate,
    arguments: Vec<String>,
    probe_args: Vec<String>,
    root: String,
    cwd_override: Option<String>,
    dependencies: String,
    router_environment: std::collections::BTreeMap<String, String>,
    approved: bool,
    probe: bool,
}

#[tauri::command]
pub async fn register_runtime(app: AppHandle, request: Registration) -> Result<Runtime> {
    blocking("registry.registration_failed", move || {
        if !request.approved {
            return Err("Review and explicitly approve the executable first".into());
        }
        if request.name.trim().is_empty() {
            return Err("Enter a runtime name".into());
        }
        adapter::validate_router_environment(&request.router_environment)?;
        if request.agent == Agent::Claude && !request.router_environment.is_empty() {
            return Err("Claude Code uses its built-in router environment mapping".into());
        }
        let candidate = registry::verify(&request.candidate)?;
        if request
            .arguments
            .iter()
            .chain(&request.probe_args)
            .any(|arg| arg.contains('\0'))
        {
            return Err("Argument contains an invalid NUL character".into());
        }
        let cwd_override = request
            .cwd_override
            .map(|path| registry::directory(&path).map(|p| p.to_string_lossy().into_owned()))
            .transpose()?;
        let root = if request.root.is_empty() {
            std::path::Path::new(&candidate.path)
                .parent()
                .ok_or("Executable parent missing")?
                .to_string_lossy()
                .into_owned()
        } else {
            registry::directory(&request.root)?
                .to_string_lossy()
                .into_owned()
        };
        let version = if request.probe {
            registry::version(&candidate, &request.probe_args)?
        } else {
            None
        };
        let runtime = Runtime {
            id: id(),
            agent: request.agent,
            name: request.name.trim().into(),
            version: version.clone().unwrap_or_else(|| "Unverified".into()),
            executable: registry::verify(&candidate)?,
            arguments: request.arguments,
            root,
            cwd_override,
            dependencies: request.dependencies,
            router_environment: request.router_environment,
            trusted: true,
            verified: version.is_some(),
            last_verified_at: request.probe.then(now),
            created_at: now(),
        };
        app.state::<Deck>()
            .store
            .put(Table::Runtimes, &runtime.id, &runtime)?;
        diagnostics::record(
            "INFO",
            "registry.runtime_registered",
            Some(&runtime.id),
            None,
            Some(if runtime.verified {
                "verified"
            } else {
                "unverified"
            }),
        );
        Ok(runtime)
    })
    .await
}

#[tauri::command]
pub fn delete_runtime(deck: State<'_, Deck>, id: String) -> Result<()> {
    deck.store.delete(Table::Runtimes, &id)
}

#[tauri::command]
pub fn save_router(deck: State<'_, Deck>, mut profile: Router) -> Result<Router> {
    diagnostics::stage(router::validate(&profile), "router.profile_invalid", None)?;
    let previous = deck.store.get::<Router>(Table::Routers, &profile.id)?;
    if let Some(previous) = previous {
        profile.created_at = previous.created_at;
        profile.updated_at = now().max(previous.updated_at.saturating_add(1));
    } else {
        profile.id = id();
        profile.created_at = now();
        profile.updated_at = profile.created_at;
    }
    diagnostics::stage(
        deck.store.put(Table::Routers, &profile.id, &profile),
        "router.profile_save_failed",
        Some(&profile.id),
    )?;
    Ok(profile)
}

#[tauri::command]
pub fn delete_router(deck: State<'_, Deck>, id: String) -> Result<()> {
    deck.store.delete(Table::Routers, &id)
}
#[tauri::command]
pub async fn probe_router(profile: Router, token: String) -> Result<router::Probe> {
    blocking("router.probe_failed", move || {
        router::test(&profile, &token)
    })
    .await
}

#[tauri::command]
pub fn save_preferences(deck: State<'_, Deck>, font_size: u16, scrollback: u32) -> Result<()> {
    let mut preferences = prefs(&deck)?;
    preferences.font_size = font_size.clamp(10, 26);
    preferences.scrollback = scrollback.clamp(100, 10000);
    deck.store.put(Table::Settings, "terminal", &preferences)
}

#[tauri::command]
pub fn import_settings(
    deck: State<'_, Deck>,
    font_size: u16,
    scrollback: u32,
    mut profiles: Vec<Router>,
) -> Result<()> {
    if profiles.len() > 100 {
        return Err("Import supports at most 100 router profiles".into());
    }
    for profile in &mut profiles {
        router::validate(profile)?;
        profile.id = id();
        profile.enabled = false;
        profile.created_at = now();
        profile.updated_at = profile.created_at;
    }
    let mut preferences = prefs(&deck)?;
    preferences.font_size = font_size.clamp(10, 26);
    preferences.scrollback = scrollback.clamp(100, 10000);
    deck.store.import_settings(&preferences, &profiles)
}

#[tauri::command]
pub async fn start_session(
    app: AppHandle,
    window: WebviewWindow,
    launch: Launch,
) -> Result<Session> {
    blocking("session.launch_failed", move || {
        let deck = app.state::<Deck>();
        if uuid::Uuid::parse_str(&launch.id).is_err() || launch.name.trim().is_empty() {
            return Err("A valid session ID and name are required".into());
        }
        diagnostics::info("session.launch_requested", Some(&launch.id));
        if deck
            .store
            .get::<Session>(Table::Sessions, &launch.id)?
            .is_some()
        {
            return Err("Session ID is already in use".into());
        }
        let runtime: Runtime = diagnostics::stage(
            required(
                &deck,
                Table::Runtimes,
                &launch.runtime_id,
                "Runtime was deleted or is unavailable",
            ),
            "session.runtime_unavailable",
            Some(&launch.id),
        )?;
        if !runtime.trusted {
            return Err("Runtime has not been approved".into());
        }
        let executable = diagnostics::stage(
            registry::verify(&runtime.executable),
            "session.executable_validation_failed",
            Some(&launch.id),
        )?;
        let cwd = diagnostics::stage(
            registry::directory(runtime.cwd_override.as_deref().unwrap_or(&launch.cwd)),
            "session.working_directory_invalid",
            Some(&launch.id),
        )?;
        if let Some(previous_id) = &launch.resume_from {
            let previous: Session = required(&deck, Table::Sessions, previous_id, "Saved session was deleted")?;
            if previous.agent != Agent::Gajae || runtime.agent != Agent::Gajae || previous.runtime_id != runtime.id {
                return Err("Resume requires the original Gajae-Code runtime".into());
            }
            if registry::directory(&previous.cwd)? != cwd {
                return Err("Resume requires the original working directory".into());
            }
            if matches!(deck.terminal.status(previous_id)?, "running" | "stopping") {
                return Err("Close the original terminal before resuming its conversation".into());
            }
        }
        let profile: Option<Router> = launch
            .router_id
            .as_ref()
            .map(|id| required(&deck, Table::Routers, id, "Router profile was deleted"))
            .transpose()?;
        let mut binding = diagnostics::stage(
            adapter::binding(runtime.agent, &launch, profile.as_ref()),
            "session.router_configuration_failed",
            Some(&launch.id),
        )?;
        adapter::apply_router_environment(&mut binding, &runtime.router_environment);
        let mut command = CommandBuilder::new(&executable.path);
        for argument in runtime.arguments.iter().chain(&launch.extra) {
            if argument.contains('\0') {
                return Err("Argument contains an invalid NUL character".into());
            }
            if launch.resume_from.is_some()
                && matches!(argument.split('=').next().unwrap_or(argument), "--" | "-r" | "--resume" | "--session" |
                    "--fork" | "-c" | "--continue" | "-p" | "--print" | "--mode" | "--no-session")
            {
                return Err("Remove session or launch-mode flags from the Gajae-Code arguments before using Resume".into());
            }
            command.arg(argument);
        }
        if launch.resume_from.is_some() {
            command.arg("--resume");
        }
        command.cwd(&cwd);
        for (name, value) in binding.env {
            command.env(name, value);
        }
        if runtime.agent == Agent::Gajae {
            // GJC's host status protocol is opt-in and older releases ignore the variable.
            command.env("GJC_HOST_STATUS", "terax");
        }
        let snapshot = Session {
            id: launch.id,
            name: launch.name.trim().into(),
            runtime_id: runtime.id,
            agent: runtime.agent,
            version: runtime.version,
            executable,
            cwd: cwd.to_string_lossy().into_owned(),
            router_id: launch.router_id,
            endpoint: binding.endpoint,
            router_revision: profile.map(|p| p.updated_at),
            model: binding.model,
            created_at: now(),
            resume_source_id: launch.resume_from,
        };
        deck.terminal.start(
            snapshot.id.clone(),
            command,
            (launch.cols, launch.rows),
            move |event| {
                let _ = window.emit("terminal-ready", event);
            },
        )?;
        if let Err(e) = deck.store.put(Table::Sessions, &snapshot.id, &snapshot) {
            diagnostics::warn("session.snapshot_save_failed", Some(&snapshot.id));
            let _ = deck.terminal.close(&snapshot.id);
            return Err(e);
        }
        if let Ok(mut preferences) = prefs(&deck) {
            preferences.last_runtime = snapshot.runtime_id.clone();
            preferences.last_cwd = snapshot.cwd.clone();
            let _ = deck.store.put(Table::Settings, "terminal", &preferences);
        }
        Ok(snapshot)
    })
    .await
}

#[tauri::command]
pub fn terminal_events(deck: State<'_, Deck>, id: String) -> Result<Vec<Event>> {
    deck.terminal.drain(&id)
}
#[tauri::command]
pub fn write_terminal(deck: State<'_, Deck>, id: String, data: String) -> Result<()> {
    diagnostics::stage(
        deck.terminal.write(&id, data),
        "terminal.write_failed",
        Some(&id),
    )
}
#[tauri::command]
pub fn resize_terminal(deck: State<'_, Deck>, id: String, cols: u16, rows: u16) -> Result<()> {
    diagnostics::stage(
        deck.terminal.resize(&id, cols, rows),
        "terminal.resize_failed",
        Some(&id),
    )
}
#[tauri::command]
pub fn status(deck: State<'_, Deck>, id: String) -> Result<&'static str> {
    deck.terminal.status(&id)
}
#[tauri::command]
pub async fn close_terminal(app: AppHandle, id: String) -> Result<()> {
    blocking("terminal.close_failed", move || {
        app.state::<Deck>().terminal.close(&id)
    })
    .await
}
#[tauri::command]
pub fn delete_session(deck: State<'_, Deck>, id: String) -> Result<()> {
    if matches!(deck.terminal.status(&id)?, "running" | "stopping") {
        return Err("Close the running terminal first".into());
    }
    deck.store.delete(Table::Sessions, &id)
}

#[tauri::command]
pub async fn resolve_session(app: AppHandle, id: String) -> Result<()> {
    blocking("session.restore_validation_failed", move || {
        let deck = app.state::<Deck>();
        let session: Session = required(&deck, Table::Sessions, &id, "Session no longer exists")?;
        let runtime: Runtime = required(
            &deck,
            Table::Runtimes,
            &session.runtime_id,
            "Saved runtime was deleted",
        )?;
        registry::directory(&session.cwd)?;
        registry::verify(&runtime.executable)?;
        if let Some(id) = session.router_id {
            let router: Router = required(&deck, Table::Routers, &id, "Saved router was deleted")?;
            if !router.enabled {
                return Err("Saved router is disabled".into());
            }
        }
        Ok(())
    })
    .await
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UiFault {
    ScriptError,
    UnhandledRejection,
}

#[tauri::command]
pub fn report_ui_fault(kind: UiFault) {
    diagnostics::warn(
        match kind {
            UiFault::ScriptError => "ui.script_error",
            UiFault::UnhandledRejection => "ui.unhandled_rejection",
        },
        None,
    );
}

#[tauri::command]
pub async fn quit(window: WebviewWindow, app: AppHandle) -> Result<()> {
    blocking("app.quit_failed", move || {
        diagnostics::info("app.quit_requested", None);
        let deck = app.state::<Deck>();
        deck.terminal.shutdown()?;
        deck.allow_close.store(true, Ordering::Release);
        window.close().map_err(error)
    })
    .await
}
