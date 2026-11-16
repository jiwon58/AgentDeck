use agent_deck::{
    adapter, diagnostics, gjc, install,
    model::{
        error, id, now, Agent, Candidate, Launch, Overview, Preferences, Result, Root, Router,
        Runtime, Session, SessionLayout,
    },
    registry, router,
    storage::{Store, Table},
    terminal::{Event, Terminal},
};
use portable_pty::CommandBuilder;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use tauri::{AppHandle, Emitter, Manager, State, WebviewWindow};

pub struct Deck {
    pub store: Store,
    pub terminal: Arc<Terminal>,
    pub directory: String,
    pub allow_close: AtomicBool,
    pub configuration: Mutex<()>,
    pub hermes_download: Mutex<Option<install::HermesReceipt>>,
}

#[tauri::command]
pub async fn download_agent(
    app: AppHandle,
    agent: Agent,
    directory: String,
) -> Result<install::Download> {
    blocking("installer.download_failed", move || match agent {
        Agent::Claude => install::claude(&directory),
        Agent::Hermes => {
            let deck = app.state::<Deck>();
            let (download, receipt) = install::hermes(&directory)?;
            *deck.hermes_download.lock().map_err(error)? = Some(receipt);
            Ok(download)
        }
        _ => Err("No publisher download is configured for this agent".into()),
    })
    .await
}

#[tauri::command]
pub async fn install_hermes(app: AppHandle, sha256: String) -> Result<registry::Scan> {
    blocking("installer.hermes_failed", move || {
        let deck = app.state::<Deck>();
        let receipt = deck
            .hermes_download
            .lock()
            .map_err(error)?
            .take()
            .ok_or("Download and review the Hermes installer in this Deck instance first")?;
        install::install_hermes(&receipt, &sha256)
    })
    .await
}

fn prefs(deck: &Deck) -> Result<Preferences> {
    Ok(deck
        .store
        .get(Table::Settings, "terminal")?
        .unwrap_or_default())
}

fn session_layout(deck: &Deck) -> Result<SessionLayout> {
    Ok(deck
        .store
        .get(Table::Settings, "session_layout")?
        .unwrap_or_default())
}

#[tauri::command]
pub fn set_session_pin(deck: State<'_, Deck>, id: String, pinned: bool) -> Result<SessionLayout> {
    let _configuration = deck.configuration.lock().map_err(error)?;
    required::<Session>(
        &deck,
        Table::Sessions,
        &id,
        "Save this session before pinning it",
    )?;
    let mut layout = session_layout(&deck)?;
    if pinned {
        layout.pinned.insert(id);
    } else {
        layout.pinned.remove(&id);
    }
    deck.store.put(Table::Settings, "session_layout", &layout)?;
    Ok(layout)
}

#[tauri::command]
pub fn set_session_grouping(deck: State<'_, Deck>, grouped: bool) -> Result<SessionLayout> {
    let _configuration = deck.configuration.lock().map_err(error)?;
    let mut layout = session_layout(&deck)?;
    layout.grouped = grouped;
    deck.store.put(Table::Settings, "session_layout", &layout)?;
    Ok(layout)
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
        gjc_presets: deck.store.list(Table::GjcPresets)?,
        preferences: prefs(&deck)?,
        session_layout: session_layout(&deck)?,
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
    let _configuration = deck.configuration.lock().map_err(error)?;
    deck.store.delete(Table::Runtimes, &id)
}

#[tauri::command]
pub fn save_router(deck: State<'_, Deck>, mut profile: Router) -> Result<Router> {
    let _configuration = deck.configuration.lock().map_err(error)?;
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
    let _configuration = deck.configuration.lock().map_err(error)?;
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
pub async fn list_router_models(
    profile: Router,
    token: String,
    protocol: router::ModelProtocol,
) -> Result<router::ModelCatalog> {
    blocking("router.model_catalog_failed", move || {
        router::list_models(&profile, &token, protocol)
    })
    .await
}

#[tauri::command]
pub async fn test_router_model(
    profile: Router,
    token: String,
    protocol: router::ModelProtocol,
    model: String,
) -> Result<router::ModelTest> {
    blocking("router.model_test_failed", move || {
        router::test_model(&profile, &token, protocol, &model)
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
pub async fn save_gjc_preset(app: AppHandle, mut preset: gjc::GjcPreset) -> Result<gjc::GjcPreset> {
    blocking("gjc.preset_save_failed", move || {
        let deck = app.state::<Deck>();
        let _configuration = deck.configuration.lock().map_err(error)?;
        if preset.id.is_empty() {
            preset.id = id();
            preset.created_at = now();
            preset.revision = 1;
        } else {
            let old: gjc::GjcPreset = required(
                &deck,
                Table::GjcPresets,
                &preset.id,
                "The preset was deleted",
            )?;
            if old.revision != preset.revision {
                return Err("The preset changed. Reopen it before saving.".into());
            }
            preset.created_at = old.created_at;
            preset.revision = old
                .revision
                .checked_add(1)
                .ok_or("Preset revision overflow")?;
        }
        preset.updated_at = now();
        preset.validate()?;
        let (runtime, profile) = gjc_refs(&deck, &preset)?;
        gjc::validate_runtime(&preset, &runtime, profile.as_ref())?;
        registry::verify(&runtime.executable)?;
        deck.store.put(Table::GjcPresets, &preset.id, &preset)?;
        Ok(preset)
    })
    .await
}

#[tauri::command]
pub fn delete_gjc_preset(deck: State<'_, Deck>, id: String, revision: u64) -> Result<()> {
    let _configuration = deck.configuration.lock().map_err(error)?;
    let preset: gjc::GjcPreset = required(&deck, Table::GjcPresets, &id, "The preset was deleted")?;
    if preset.revision != revision {
        return Err("The preset changed. Refresh before deleting.".into());
    }
    deck.store.delete(Table::GjcPresets, &id)
}

fn gjc_refs(deck: &Deck, preset: &gjc::GjcPreset) -> Result<(Runtime, Option<Router>)> {
    let runtime = required(
        deck,
        Table::Runtimes,
        &preset.runtime_id,
        "Preset runtime is missing",
    )?;
    let profile = preset
        .router_id
        .as_deref()
        .map(|id| required(deck, Table::Routers, id, "Preset router is missing"))
        .transpose()?;
    Ok((runtime, profile))
}

fn resolve_gjc(
    deck: &Deck,
    launch: &Launch,
    selection: &gjc::Selection,
) -> Result<gjc::ResolvedGjcLaunch> {
    let preset: gjc::GjcPreset = required(
        deck,
        Table::GjcPresets,
        &selection.preset_id,
        "The GJC preset was deleted",
    )?;
    if uuid::Uuid::parse_str(&preset.id).is_err() {
        return Err("Invalid preset identity".into());
    }
    let (runtime, profile) = gjc_refs(deck, &preset)?;
    gjc::validate_runtime(&preset, &runtime, profile.as_ref())?;
    registry::verify(&runtime.executable)?;
    let cwd = registry::directory(&launch.cwd)?;
    let mut previous = None;
    if let Some(id) = &launch.resume_from {
        let session: Session = required(deck, Table::Sessions, id, "Saved session was deleted")?;
        if session.runtime_id != runtime.id
            || session.agent != Agent::Gajae
            || registry::directory(&session.cwd)? != cwd
            || session.executable.sha256 != runtime.executable.sha256
        {
            return Err(
                "Resume requires the original approved GJC runtime and working directory".into(),
            );
        }
        if deck.terminal.history_active(id)? {
            return Err("Close the original terminal before resuming its conversation".into());
        }
        if session
            .gjc
            .as_ref()
            .map(|history| history.preset_id.as_str())
            != Some(preset.id.as_str())
        {
            return Err(
                "Resume requires the original preset. Use manual Resume for earlier sessions."
                    .into(),
            );
        }
        previous = session.gjc;
    }
    let app_data = registry::directory(&deck.directory)?;
    let inputs = gjc::filesystem_inputs(
        &app_data,
        &cwd,
        &preset,
        &runtime,
        launch,
        selection,
        previous.as_ref(),
    )?;
    let plan = gjc::resolve(
        &preset,
        &runtime,
        profile.as_ref(),
        launch,
        selection,
        inputs,
    )?;
    gjc::validate_targets(&plan)?;
    Ok(plan)
}

#[derive(serde::Serialize)]
pub struct GjcPreview {
    fingerprint: String,
    plan: gjc::ResolvedGjcLaunch,
}

#[tauri::command]
pub async fn preview_gjc_preset(app: AppHandle, launch: Launch) -> Result<GjcPreview> {
    blocking("gjc.preview_failed", move || {
        let deck = app.state::<Deck>();
        let _configuration = deck.configuration.lock().map_err(error)?;
        let selection = launch.gjc.as_ref().ok_or("Select a GJC preset")?;
        let plan = resolve_gjc(&deck, &launch, selection)?;
        Ok(GjcPreview {
            fingerprint: plan.fingerprint()?,
            plan,
        })
    })
    .await
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
        let _configuration = deck.configuration.lock().map_err(error)?;
        if let Some(selection) = &launch.gjc {
            let plan = resolve_gjc(&deck, &launch, selection)?;
            if plan.fingerprint()? != selection.preview_fingerprint {
                diagnostics::warn("gjc.preview_stale", Some(&launch.id));
                return Err("The launch configuration changed. Review a new preview before starting.".into());
            }
            let credential = plan.credential(&launch.token)?;
            let prepared = diagnostics::stage(gjc::prepare(&plan), "gjc.prepare_failed", Some(&launch.id))?;
            let mut command = CommandBuilder::new(&plan.executable);
            for argument in &plan.argv { command.arg(argument); }
            for (key, value) in &plan.env { command.env(key, value); }
            if let Some((key, value)) = credential { command.env(key, value); }
            command.cwd(&plan.cwd);
            let runtime: Runtime = required(&deck, Table::Runtimes, &plan.runtime_id, "Preset runtime is missing")?;
            let executable = registry::verify(&runtime.executable)?;
            let snapshot = Session {
                id: launch.id, name: launch.name.trim().into(), runtime_id: plan.runtime_id,
                agent: Agent::Gajae, version: plan.version, executable, cwd: plan.cwd,
                router_id: plan.router_id, endpoint: plan.endpoint, router_revision: plan.router_revision,
                model: plan.model, created_at: now(), resume_source_id: launch.resume_from, gjc: Some(plan.history),
            };
            return record_launch(&deck, window, snapshot, command, (launch.cols, launch.rows), prepared);
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
            if previous.gjc.is_some() {
                return Err("Select the original GJC preset to resume this session".into());
            }
            if previous.agent != Agent::Gajae || runtime.agent != Agent::Gajae || previous.runtime_id != runtime.id {
                return Err("Resume requires the original Gajae-Code runtime".into());
            }
            if registry::directory(&previous.cwd)? != cwd {
                return Err("Resume requires the original working directory".into());
            }
            if deck.terminal.history_active(previous_id)? {
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
            gjc: None,
        };
        record_launch(&deck, window, snapshot, command, (launch.cols, launch.rows), gjc::Prepared::default())
    })
    .await
}

fn record_launch(
    deck: &Deck,
    window: WebviewWindow,
    snapshot: Session,
    command: CommandBuilder,
    size: (u16, u16),
    prepared: gjc::Prepared,
) -> Result<Session> {
    deck.terminal.start_with_lease(
        snapshot.id.clone(),
        snapshot.resume_source_id.as_deref(),
        command,
        size,
        move |event| {
            let _ = window.emit("terminal-ready", event);
        },
        prepared.lease,
    )?;
    drop(prepared.writer);
    // A resume gets a fresh process snapshot while its saved history row keeps its identity.
    if let Err(e) = snapshot
        .resume_source_id
        .is_none()
        .then(|| deck.store.put(Table::Sessions, &snapshot.id, &snapshot))
        .transpose()
    {
        diagnostics::warn("session.snapshot_save_failed", Some(&snapshot.id));
        let _ = deck.terminal.close(&snapshot.id);
        return Err(e);
    }
    if let Ok(mut preferences) = prefs(deck) {
        preferences.last_runtime = snapshot.runtime_id.clone();
        preferences.last_cwd = snapshot.cwd.clone();
        let _ = deck.store.put(Table::Settings, "terminal", &preferences);
    }
    Ok(snapshot)
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
    let _configuration = deck.configuration.lock().map_err(error)?;
    if deck.terminal.history_active(&id)? {
        return Err("Close the running terminal first".into());
    }
    let mut layout = session_layout(&deck)?;
    if layout.pinned.remove(&id) {
        deck.store.put(Table::Settings, "session_layout", &layout)?;
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
        if let Some(history) = session.gjc {
            let preset: gjc::GjcPreset = required(
                &deck,
                Table::GjcPresets,
                &history.preset_id,
                "Saved GJC preset was deleted",
            )?;
            let (runtime, profile) = gjc_refs(&deck, &preset)?;
            gjc::validate_runtime(&preset, &runtime, profile.as_ref())?;
        } else if let Some(id) = session.router_id {
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
