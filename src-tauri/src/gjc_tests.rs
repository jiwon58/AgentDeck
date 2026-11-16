use super::*;
use crate::model::Candidate;
use models::read_models;
use serde_json::Value;
use std::fs;

fn directory(path: &str) -> PathBuf {
    if cfg!(windows) {
        Path::new("C:\\").join(path)
    } else {
        Path::new("/").join(path)
    }
}

fn preset() -> GjcPreset {
    GjcPreset {
        id: crate::model::id(),
        name: "Preset A".into(),
        enabled: true,
        auto_config_enabled: true,
        revision: 1,
        model: "mock-local".into(),
        provider_id: "example-local".into(),
        runtime_id: "runtime-a".into(),
        router_id: Some("loopback".into()),
        directory_mode: DirectoryMode::Managed,
        context_tokens: 8192,
        max_output_tokens: 1024,
        stream_timeout: Toggle {
            enabled: true,
            value: "900000".into(),
        },
        ui_language: Toggle {
            enabled: true,
            value: "ko".into(),
        },
        model_preset_registry: Toggle {
            enabled: true,
            value: "1".into(),
        },
        ..Default::default()
    }
}

fn runtime(version: &str) -> Runtime {
    Runtime {
        id: "runtime-a".into(),
        name: "GJC".into(),
        version: version.into(),
        agent: Agent::Gajae,
        executable: Candidate {
            path: directory("gjc.exe").display().to_string(),
            sha256: "hash".into(),
            architecture: "x64".into(),
            launch_type: "native".into(),
        },
        arguments: vec![],
        root: "root".into(),
        cwd_override: None,
        dependencies: "".into(),
        trusted: true,
        verified: true,
        last_verified_at: None,
        created_at: 0,
        router_environment: BTreeMap::new(),
    }
}

fn router() -> Router {
    Router {
        id: "loopback".into(),
        name: "Mock".into(),
        enabled: true,
        scheme: "http".into(),
        host: "127.0.0.1".into(),
        port: 9011,
        allow_remote: false,
        openai_path: Some("/v1".into()),
        anthropic_path: Some("/custom/v1".into()),
        model: "router-default".into(),
        authentication: "bearer".into(),
        created_at: 0,
        updated_at: 0,
    }
}

fn resolve_for(
    preset: &GjcPreset,
    runtime: &Runtime,
    profile: &Router,
    app: &Path,
    cwd: &Path,
    launch: &Launch,
    history: Option<PathBuf>,
) -> Result<ResolvedGjcLaunch> {
    resolve(
        preset,
        runtime,
        Some(profile),
        launch,
        &Selection::default(),
        Inputs {
            app_data: app,
            cwd,
            agent_directory: Some(app.join(&preset.id)),
            session_directory: history,
            warning: None,
            previous: None,
            native_history: false,
            inject_agent_directory: true,
            inherited_no_proxy: [None, None],
        },
    )
}

#[test]
fn deterministic_plan_has_cwd_independent_config() {
    let mut preset = preset();
    let mut runtime = runtime("0.20.0");
    let mut router = router();
    let app = directory("app");
    let cwd = directory("workspace A");
    let cwd2 = directory("workspace B");
    let launch = Launch::default();
    let a = resolve_for(
        &preset,
        &runtime,
        &router,
        &app,
        &cwd,
        &launch,
        Some(cwd.join("history")),
    )
    .unwrap();
    let a2 = resolve_for(
        &preset,
        &runtime,
        &router,
        &app,
        &cwd,
        &launch,
        Some(cwd.join("history")),
    )
    .unwrap();
    let b = resolve_for(
        &preset,
        &runtime,
        &router,
        &app,
        &cwd2,
        &launch,
        Some(cwd2.join("history")),
    )
    .unwrap();
    assert_eq!(a.fingerprint().unwrap(), a2.fingerprint().unwrap());
    assert_eq!(
        (a.models_target.as_ref(), &a.models, &a.env, &a.argv[..3]),
        (b.models_target.as_ref(), &b.models, &b.env, &b.argv[..3])
    );
    assert_ne!(a.history.session_directory, b.history.session_directory);
    assert_ne!(a.cwd, b.cwd);
    assert_eq!(a.endpoint.as_deref(), Some("http://127.0.0.1:9011/v1"));
    assert_eq!(a.env[TIMEOUT], "900000");
    assert_eq!(a.env[LANGUAGE], "ko");
    assert_eq!(a.env[REGISTRY], "1");
    assert!(!serde_json::to_string(&a).unwrap().contains(KEY));
    assert_eq!(
        a.credential("temporary credential").unwrap().unwrap().0,
        KEY
    );
    assert!(a.credential("").is_err());

    let first = a.fingerprint().unwrap();
    preset.revision += 1;
    let update = resolve_for(
        &preset,
        &runtime,
        &router,
        &app,
        &cwd,
        &launch,
        Some(cwd.join("history")),
    )
    .unwrap();
    assert_ne!(first, update.fingerprint().unwrap());
    router.updated_at += 1;
    router.port += 1;
    runtime.executable.sha256 = "changed".into();
    let update2 = resolve_for(
        &preset,
        &runtime,
        &router,
        &app,
        &cwd,
        &launch,
        Some(cwd.join("history")),
    )
    .unwrap();
    assert_ne!(
        update.fingerprint().unwrap(),
        update2.fingerprint().unwrap()
    );
}

#[test]
fn protocol_tokens_and_environment() {
    let mut preset = preset();
    let runtime = runtime("0.20.0");
    let router = router();
    let root = tempfile::tempdir().unwrap();
    let cwd = dunce::canonicalize(root.path()).unwrap();
    preset.protocol = Protocol::Anthropic;
    preset.ui_language.enabled = false;
    preset.variables.push(Variable {
        enabled: true,
        key: "DECK_OPTION".into(),
        value: "old".into(),
    });
    preset.variables.push(Variable {
        enabled: false,
        key: "DECK_OFF".into(),
        value: "ignored".into(),
    });
    let launch = Launch {
        env: BTreeMap::from([
            ("deck_option".into(), "new".into()),
            (AGENT_DIR.into(), cwd.display().to_string()),
        ]),
        ..Default::default()
    };
    let selection = Selection::default();
    let inputs =
        filesystem_inputs(&cwd, &cwd, &preset, &runtime, &launch, &selection, None).unwrap();
    let plan = resolve(
        &preset,
        &runtime,
        Some(&router),
        &launch,
        &selection,
        inputs,
    )
    .unwrap();
    assert_eq!(plan.history.agent_directory.as_deref(), Some(cwd.as_path()));
    assert_eq!(plan.env["deck_option"], "new");
    assert!(!plan.env.contains_key("DECK_OFF"));
    assert!(!plan.env.contains_key(TIMEOUT));
    assert!(!plan.env.contains_key(LANGUAGE));
    let json: Value =
        serde_json::from_str(plan.models.as_ref().unwrap().split_once('\n').unwrap().1).unwrap();
    let provider = &json["providers"]["example-local"];
    assert_eq!(provider["api"], "anthropic-messages");
    assert_eq!(provider["baseUrl"], "http://127.0.0.1:9011/custom");
    assert_eq!(provider["models"][0]["id"], "mock-local");
    assert_eq!(provider["models"][0]["contextWindow"], 8192);
    assert_eq!(provider["models"][0]["maxTokens"], 1024);
    assert_eq!(
        json["modelBindings"]["modelRoles"]["default"],
        "example-local/mock-local"
    );

    preset.variables[0].key = "API_TOKEN".into();
    assert!(preset.validate().is_err());
    preset.variables[0].key = "DECK_NAME".into();
    preset.variables[0].value = "Bearer example".into();
    assert!(preset.validate().is_err());
    preset.variables[0].value = "valid".into();
    preset.variables[1].key = "deck_name".into();
    assert!(preset.validate().is_err());
}

#[test]
fn history_flags_are_parsed_as_arguments() {
    let path = directory("한글 directory");
    let string = path.display().to_string();
    let mut resume = vec!["--model".into(), "model".into(), "--resume".into()];
    assert_eq!(
        history_arguments(&mut resume, Some(&path)).unwrap(),
        Some(path.clone())
    );
    assert_eq!(
        resume,
        ["--model", "model", "--session-dir", &string, "--resume"]
    );
    assert!(explicit_history(&[
        "--session-dir".into(),
        string.clone(),
        format!("--session-dir={string}")
    ])
    .is_err());
    let mut explicit = vec![format!("--session-dir={string}"), "--resume".into()];
    assert_eq!(
        history_arguments(&mut explicit, Some(&directory("ignored"))).unwrap(),
        Some(path.clone())
    );
    assert_eq!(explicit.len(), 2);
    let mut no_session = vec!["--no-session".into()];
    assert!(history_arguments(&mut no_session, Some(&path))
        .unwrap()
        .is_none());
    assert_eq!(no_session, ["--no-session"]);
    assert!(explicit_history(&[format!("--session-dir={string}"), "--no-session".into()]).is_err());
    let mut default = vec!["--resume".into()];
    assert!(history_arguments(&mut default, None).unwrap().is_none());
    assert_eq!(default, ["--resume"]);
    let mut positional = vec!["--".into(), "--no-session".into()];
    assert_eq!(
        history_arguments(&mut positional, Some(&path)).unwrap(),
        Some(path)
    );
    assert_eq!(positional, ["--session-dir", &string, "--", "--no-session"]);
}

#[test]
fn unsupported_version_manual_preset_keeps_native_defaults() {
    let mut preset = preset();
    let runtime = runtime("9.99.99");
    let profile = router();
    assert!(validate_runtime(&preset, &runtime, Some(&profile)).is_err());
    preset.auto_config_enabled = false;
    preset.history_mode = HistoryMode::ExistingWorkspace;
    let app = tempfile::tempdir().unwrap();
    let path = app.path();
    let launch = Launch::default();
    let selection = Selection::default();
    let inputs =
        filesystem_inputs(path, path, &preset, &runtime, &launch, &selection, None).unwrap();
    let plan = resolve(
        &preset,
        &runtime,
        Some(&profile),
        &launch,
        &selection,
        inputs,
    )
    .unwrap();
    assert_eq!(plan.history.history_mode, HistoryMode::Default);
    assert!(plan.history.session_directory.is_none());
    assert!(plan.argv.is_empty());
    assert!(plan.models_target.is_none());
    assert!(!plan.env.contains_key(AGENT_DIR));
    assert!(scope::v2_identity(Path::new("nonexistent")).is_none());
}

#[test]
fn live_leases_protect_models_and_changes_backup() {
    let mut preset = preset();
    let app = tempfile::tempdir().unwrap();
    let app = dunce::canonicalize(app.path()).unwrap();
    let cwd = directory("project");
    let runtime = runtime("0.20.0");
    let router = router();
    let launch = Launch::default();
    let first = resolve_for(&preset, &runtime, &router, &app, &cwd, &launch, None).unwrap();
    let Prepared {
        lease: session,
        writer,
    } = prepare(&first).unwrap();
    drop(writer);
    let target = first.models_target.as_ref().unwrap();
    let modified = fs::metadata(target).unwrap().modified().unwrap();
    let reread = read_models(target).unwrap();
    let Prepared {
        lease: second,
        writer,
    } = prepare(&first).unwrap();
    drop(writer);
    assert_eq!(fs::metadata(target).unwrap().modified().unwrap(), modified);
    preset.model = "another-model".into();
    let changed = resolve_for(&preset, &runtime, &router, &app, &cwd, &launch, None).unwrap();
    assert!(prepare(&changed).err().unwrap().contains("live session"));
    assert_eq!(read_models(target).unwrap(), reread);
    drop(session);
    assert!(prepare(&changed).is_err());
    drop(second);
    drop(prepare(&changed).unwrap());
    assert_eq!(read_models(target).unwrap(), changed.models);
    let backup = fs::read_dir(target.parent().unwrap())
        .unwrap()
        .flatten()
        .find(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with("models.yml.bak.")
        })
        .unwrap();
    assert_eq!(read_models(&backup.path()).unwrap(), reread);
    fs::write(target, "providers:\n  manually-configured: {}\n").unwrap();
    assert!(validate_targets(&changed)
        .unwrap_err()
        .contains("user configuration"));
    assert!(prepare(&changed)
        .err()
        .unwrap()
        .contains("user configuration"));
    assert!(fs::read_to_string(target)
        .unwrap()
        .contains("manually-configured"));
}

#[test]
fn legacy_json_defaults_to_manual_launch() {
    let launch: Launch = serde_json::from_str(r#"{"name":"Already saved"}"#).unwrap();
    let preset: GjcPreset = serde_json::from_str(r#"{"name":"Draft"}"#).unwrap();
    assert!(launch.gjc.is_none() && launch.env.is_empty());
    assert!(!preset.auto_config_enabled);
    assert_eq!(preset.history_mode, HistoryMode::Default);
}

#[test]
fn legacy_and_v2_source_vectors() {
    let encode = |cwd| scope::legacy(r"C:\Users\alice", r"C:\Temp", cwd, true);
    assert_eq!(encode(r"C:\Users\alice"), "-");
    assert_eq!(encode(r"C:\Users\alice\project"), "-project");
    assert_eq!(encode(r"D:\Code\App"), "--D--Code-App--");
    assert_eq!(encode(r"c:/users/ALICE/한글 folder"), "-한글 folder");
    assert_eq!(encode(r"C:\Temp\proj"), "-tmp-proj");
    assert_eq!(
        encode(r"C:\Users\alice2\project"),
        "--C--Users-alice2-project--"
    );
    assert_eq!(encode(r"\\server\share\work"), "---server-share-work--");
    let identity = r"\\?\Volume{01234567-89ab-cdef-0123-456789abcdef}\Code\한글";
    assert_eq!(
        scope::v2_digest("win32", identity),
        "v2-dyjr2kcdpbsdudhzw3rphtcvur3z2f5gbtl5xgur7qxeu7j2itjq"
    );
}
