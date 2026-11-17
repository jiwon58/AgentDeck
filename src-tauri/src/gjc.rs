use crate::{
    adapter,
    model::{error, Agent, Launch, Result, Router, Runtime},
    registry, router,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

#[path = "gjc_models.rs"]
mod models;
use models::models_document;
pub use models::{prepare, validate_targets, Prepared};

#[path = "gjc_scope.rs"]
pub mod scope;

#[cfg(test)]
#[path = "gjc_tests.rs"]
mod tests;

const AGENT_DIR: &str = "GJC_CODING_AGENT_DIR";
const KEY: &str = "AGENT_DECK_GJC_ROUTER_KEY";
const TIMEOUT: &str = "GJC_OPENAI_STREAM_IDLE_TIMEOUT_MS";
const LANGUAGE: &str = "GJC_UI_LANGUAGE";
const REGISTRY: &str = "GJC_MODEL_PRESET_REGISTRY_DISABLED";
// These tags have separately reviewed CLI and models.yml contracts.
fn supported(runtime: &Runtime) -> bool {
    runtime.verified && matches!(runtime.version.as_str(), "0.10.2" | "0.20.0")
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DirectoryMode {
    #[default]
    Default,
    Managed,
    Custom,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HistoryMode {
    #[default]
    Default,
    ExistingWorkspace,
    Custom,
}

#[derive(Clone, Copy, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Protocol {
    #[default]
    Openai,
    Anthropic,
}

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Toggle {
    enabled: bool,
    value: String,
}

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(default)]
struct Variable {
    key: String,
    value: String,
    enabled: bool,
}

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct GjcPreset {
    pub id: String,
    pub name: String,
    description: String,
    pub enabled: bool,
    pub auto_config_enabled: bool,
    pub revision: u64,
    pub created_at: u64,
    pub updated_at: u64,
    pub runtime_id: String,
    pub router_id: Option<String>,
    protocol: Protocol,
    provider_id: String,
    model: String,
    context_tokens: u32,
    max_output_tokens: u32,
    directory_mode: DirectoryMode,
    agent_path: String,
    history_mode: HistoryMode,
    history_path: String,
    stream_timeout: Toggle,
    ui_language: Toggle,
    model_preset_registry: Toggle,
    variables: Vec<Variable>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
pub struct Selection {
    pub preset_id: String,
    pub preview_fingerprint: String,
    model: Option<String>,
    stream_timeout: Option<String>,
    history_mode: Option<HistoryMode>,
    history_path: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct HistorySnapshot {
    pub preset_id: String,
    pub preset_revision: u64,
    pub agent_directory: Option<PathBuf>,
    pub session_directory: Option<PathBuf>,
    pub history_mode: HistoryMode,
    pub models_fingerprint: Option<String>,
    #[serde(default)]
    pub history_disabled: bool,
    #[serde(default)]
    native_history: bool,
}

#[derive(Serialize)]
pub struct ResolvedGjcLaunch {
    pub history: HistorySnapshot,
    pub runtime_id: String,
    pub version: String,
    pub executable: String,
    pub executable_sha256: String,
    pub cwd: String,
    pub router_id: Option<String>,
    pub router_revision: Option<u64>,
    pub endpoint: Option<String>,
    pub provider_id: String,
    pub protocol: Protocol,
    pub model: String,
    pub argv: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub models_target: Option<PathBuf>,
    pub managed_startup_config: bool,
    pub warnings: Vec<String>,
    pub requires_token: bool,
    #[serde(skip)]
    models: Option<String>,
}

pub struct Inputs<'a> {
    pub app_data: &'a Path,
    pub cwd: &'a Path,
    pub agent_directory: Option<PathBuf>,
    pub session_directory: Option<PathBuf>,
    pub warning: Option<&'static str>,
    pub previous: Option<&'a HistorySnapshot>,
    pub native_history: bool,
    pub inject_agent_directory: bool,
    pub inherited_no_proxy: [Option<String>; 2],
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

impl ResolvedGjcLaunch {
    pub fn fingerprint(&self) -> Result<String> {
        Ok(digest(&serde_json::to_vec(self).map_err(error)?))
    }

    pub fn credential(&self, token: &str) -> Result<Option<(&'static str, String)>> {
        if self.requires_token {
            if token.trim().is_empty() || token.chars().any(char::is_control) {
                return Err("Enter a valid router credential for this launch".into());
            }
            return Ok(Some((KEY, token.to_owned())));
        }
        Ok(self
            .models
            .as_ref()
            .map(|_| (KEY, "agent-deck-local".into())))
    }
}

fn safe_value(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    !value.chars().any(char::is_control)
        && ![
            "bearer ",
            "basic ",
            "ghp_",
            "github_pat_",
            "-----begin ",
            "://",
        ]
        .iter()
        .any(|pattern| lower.contains(pattern))
        && !lower
            .split(|c: char| c.is_whitespace() || [':', '=', '/', '\\'].contains(&c))
            .any(|part| part.starts_with("sk-") && part.len() >= 15)
}

fn variable_name(key: &str) -> bool {
    !key.is_empty()
        && !key.as_bytes()[0].is_ascii_digit()
        && key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

fn safe_argument(arg: &str) -> bool {
    let key = arg
        .split('=')
        .next()
        .unwrap_or_default()
        .trim_start_matches('-')
        .to_ascii_uppercase()
        .replace('-', "_");
    let secret_flag = arg.starts_with('-')
        && key != "MAX_TOKENS"
        && [
            "TOKEN",
            "PASSWORD",
            "SECRET",
            "AUTH",
            "API_KEY",
            "COOKIE",
            "CREDENTIAL",
        ]
        .iter()
        .any(|part| key.contains(part));
    !secret_flag && safe_value(arg)
}

fn persistable_key(key: &str) -> bool {
    let upper = key.to_ascii_uppercase();
    variable_name(key)
        && ![
            AGENT_DIR,
            KEY,
            TIMEOUT,
            LANGUAGE,
            REGISTRY,
            "GJC_HOST_STATUS",
            "GJC_CONFIG_DIR",
            "GJC_AGENT_DIR",
            "GJC_SESSION_DIR",
            "PI_CONFIG_DIR",
            "PI_CODING_AGENT_DIR",
            "PWD",
            "HOME",
            "USERPROFILE",
            "HOMEPATH",
            "HOMEDRIVE",
            "TEMP",
            "TMP",
            "TMPDIR",
            "NO_PROXY",
            "PATH",
        ]
        .contains(&upper.as_str())
        && ![
            "TOKEN",
            "KEY",
            "PASSWORD",
            "PASSWD",
            "SECRET",
            "AUTH",
            "COOKIE",
            "CREDENTIAL",
        ]
        .iter()
        .any(|part| upper.contains(part))
}

fn absolute(value: &str) -> Result<&Path> {
    let path = Path::new(value);
    if !path.is_absolute() || value.chars().any(char::is_control) {
        return Err("Enter an absolute directory path without control characters".into());
    }
    Ok(path)
}

impl GjcPreset {
    pub fn validate(&self) -> Result<()> {
        if self.name.trim().is_empty()
            || [
                &self.name,
                &self.description,
                &self.provider_id,
                &self.model,
                &self.agent_path,
                &self.history_path,
            ]
            .into_iter()
            .any(|value| !safe_value(value))
        {
            return Err(
                "Enter a preset name and remove credentials or invalid characters from its fields"
                    .into(),
            );
        }
        if self.auto_config_enabled
            && (self.router_id.is_none()
                || self.model.trim().is_empty()
                || self.provider_id.is_empty()
                || !self
                    .provider_id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b))
                || self.context_tokens == 0
                || self.max_output_tokens == 0
                || self.max_output_tokens > self.context_tokens)
        {
            return Err("Automatic configuration requires a router, provider ID, model and valid token limits".into());
        }
        let mut keys = BTreeSet::new();
        for variable in &self.variables {
            if !persistable_key(&variable.key)
                || !keys.insert(variable.key.to_ascii_uppercase())
                || !safe_value(&variable.value)
            {
                return Err("Preset environment contains a reserved, duplicated, invalid or credential-like entry".into());
            }
        }
        for toggle in [
            &self.stream_timeout,
            &self.ui_language,
            &self.model_preset_registry,
        ] {
            if !safe_value(&toggle.value) {
                return Err(
                    "Preset environment options contain an invalid or credential-like value".into(),
                );
            }
        }
        if self.auto_config_enabled && self.directory_mode == DirectoryMode::Custom {
            absolute(&self.agent_path)?;
        }
        if self.auto_config_enabled && self.history_mode == HistoryMode::Custom {
            absolute(&self.history_path)?;
        }
        Ok(())
    }
}

pub fn validate_runtime(
    preset: &GjcPreset,
    runtime: &Runtime,
    router: Option<&Router>,
) -> Result<()> {
    if runtime.agent != Agent::Gajae || !runtime.trusted || runtime.id != preset.runtime_id {
        return Err("The preset requires an approved GJC runtime".into());
    }
    if runtime.cwd_override.is_some() {
        return Err("Use a GJC runtime without a working directory override for presets".into());
    }
    if preset.auto_config_enabled && !supported(runtime) {
        return Err("Automatic GJC configuration is verified for 0.10.2 and 0.20.0. Disable it for this runtime.".into());
    }
    if let Some(router) = router {
        crate::router::validate(router)?;
        if !router.enabled {
            return Err("The preset router is disabled".into());
        }
        endpoint(preset.protocol, router)?;
    }
    Ok(())
}

fn endpoint(protocol: Protocol, router: &Router) -> Result<String> {
    let (path, default) = match protocol {
        Protocol::Openai => (&router.openai_path, "/v1"),
        Protocol::Anthropic => (&router.anthropic_path, ""),
    };
    let path = path
        .as_deref()
        .ok_or("The preset protocol is disabled on its router")?;
    let path = if path.is_empty() { default } else { path };
    let path = match protocol {
        Protocol::Anthropic => path
            .trim_end_matches('/')
            .strip_suffix("/v1")
            .unwrap_or(path),
        Protocol::Openai => path,
    };
    router::endpoint(router, path)
}

fn override_dir(launch: &Launch) -> Option<&str> {
    launch
        .env
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(AGENT_DIR))
        .map(|(_, value)| value.as_str())
}

fn requested_history(preset: &GjcPreset, select: &Selection) -> HistoryMode {
    if preset.auto_config_enabled {
        select.history_mode.unwrap_or(preset.history_mode)
    } else {
        HistoryMode::Default
    }
}

pub fn default_agent_directory() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os(AGENT_DIR).filter(|path| !path.is_empty()) {
        return registry::directory(
            path.to_str()
                .ok_or("Inherited agent directory is not valid Unicode")?,
        );
    }
    if !cfg!(windows) {
        return Err(
            "Select a Managed or Custom directory to configure this platform's GJC storage".into(),
        );
    }
    let home = std::env::home_dir().ok_or("The GJC home directory is unavailable")?;
    let config = std::env::var("GJC_CONFIG_DIR")
        .or_else(|_| std::env::var("PI_CONFIG_DIR"))
        .unwrap_or_else(|_| ".gjc".into());
    if !matches!(
        Path::new(&config)
            .components()
            .collect::<Vec<_>>()
            .as_slice(),
        [std::path::Component::Normal(_)]
    ) {
        return Err("Select Managed or Custom when GJC uses a nonstandard config root".into());
    }
    Ok(dunce::canonicalize(home)
        .map_err(error)?
        .join(config)
        .join("agent"))
}

pub fn filesystem_inputs<'a>(
    app_data: &'a Path,
    cwd: &'a Path,
    preset: &GjcPreset,
    runtime: &Runtime,
    launch: &Launch,
    select: &Selection,
    previous: Option<&'a HistorySnapshot>,
) -> Result<Inputs<'a>> {
    let mut warning = None;
    let mut native_history = false;
    let inject_agent_directory = override_dir(launch).is_some()
        || (preset.auto_config_enabled && preset.directory_mode != DirectoryMode::Default);
    let agent_directory = if let Some(path) = override_dir(launch) {
        Some(existing_directory(path)?)
    } else if preset.auto_config_enabled {
        match preset.directory_mode {
            DirectoryMode::Managed => Some(
                app_data
                    .join("gjc")
                    .join("presets")
                    .join(&preset.id)
                    .join("agent"),
            ),
            DirectoryMode::Custom => Some(existing_directory(&preset.agent_path)?),
            DirectoryMode::Default => Some(default_agent_directory()?),
        }
    } else {
        None
    };
    if let Some(previous) = previous {
        if agent_directory != previous.agent_directory {
            return Err("Restore the original agent directory before opening its history".into());
        }
    }
    let has_manual_history = explicit_history(
        &runtime
            .arguments
            .iter()
            .chain(&launch.extra)
            .cloned()
            .collect::<Vec<_>>(),
    )?;
    let session_directory = if let Some(manual) = has_manual_history {
        manual
            .map(|directory| {
                existing_directory(
                    directory
                        .to_str()
                        .ok_or("History path is not valid Unicode")?,
                )
            })
            .transpose()?
    } else if let Some(previous) = previous {
        if let Some(path) = &previous.session_directory {
            let supplied = path.to_str().ok_or("History path is not valid Unicode")?;
            if !supported(runtime) {
                return Err("The runtime no longer supports the saved history override".into());
            }
            native_history = previous.native_history;
            if native_history {
                let expected = v2_directory(agent_directory.as_deref(), cwd)
                    .ok_or("The native workspace identity is unavailable")?;
                if expected != *path {
                    return Err(
                        "The native workspace identity changed since the original session".into(),
                    );
                }
            }
            Some(registry::directory(supplied)?)
        } else {
            None
        }
    } else {
        match requested_history(preset, select) {
            HistoryMode::Default => None,
            HistoryMode::Custom => {
                if !supported(runtime) {
                    return Err("This runtime's --session-dir flag has not been verified".into());
                }
                Some(existing_directory(
                    select
                        .history_path
                        .as_deref()
                        .unwrap_or(&preset.history_path),
                )?)
            }
            HistoryMode::ExistingWorkspace => {
                let outcome = match runtime.version.as_str() {
                    "0.10.2" if supported(runtime) => agent_directory
                        .as_deref()
                        .zip(cwd.to_str())
                        .and_then(|(agent, cwd)| {
                            scope::legacy_current(cwd)
                                .map(|scope| agent.join("sessions").join(scope))
                        }),
                    "0.20.0" if supported(runtime) => {
                        let expected = v2_directory(agent_directory.as_deref(), cwd);
                        native_history = expected.is_some();
                        expected
                    }
                    _ => None,
                };
                if outcome.is_none() {
                    warning = Some("Existing Workspace is unavailable for this runtime or path. GJC will resolve its default history.");
                }
                outcome
            }
        }
    };
    Ok(Inputs {
        app_data,
        cwd,
        agent_directory,
        session_directory,
        warning,
        previous,
        native_history,
        inject_agent_directory,
        inherited_no_proxy: adapter::inherited_no_proxy(),
    })
}

fn v2_directory(agent: Option<&Path>, cwd: &Path) -> Option<PathBuf> {
    let identity = scope::v2_identity(cwd)?;
    let platform = if cfg!(windows) { "win32" } else { "posix" };
    Some(
        agent?
            .join("sessions")
            .join(scope::v2_digest(platform, &identity)),
    )
}

fn existing_directory(value: &str) -> Result<PathBuf> {
    absolute(value)?;
    registry::directory(value)
}

fn explicit_history(argv: &[String]) -> Result<Option<Option<PathBuf>>> {
    let mut history = None;
    let mut disabled = false;
    let mut args = argv.iter();
    while let Some(arg) = args.next() {
        if arg == "--" {
            break;
        }
        if arg == "--no-session" {
            disabled = true;
        }
        let value = if arg == "--session-dir" {
            Some(
                args.next()
                    .ok_or("Provide a path after --session-dir")?
                    .as_str(),
            )
        } else {
            arg.strip_prefix("--session-dir=")
        };
        if let Some(value) = value {
            if history.is_some() {
                return Err("Only one --session-dir is allowed".into());
            }
            history = Some(absolute(value)?.to_path_buf());
        }
    }
    if disabled && history.is_some() {
        return Err("--no-session conflicts with --session-dir".into());
    }
    Ok(if disabled {
        Some(None)
    } else {
        history.map(Some)
    })
}

fn history_arguments(argv: &mut Vec<String>, directory: Option<&Path>) -> Result<Option<PathBuf>> {
    if let Some(manual) = explicit_history(argv)? {
        return Ok(manual);
    }
    if let Some(directory) = directory {
        let path = directory
            .to_str()
            .ok_or("Session directory is not valid Unicode")?;
        let position = argv
            .iter()
            .position(|arg| {
                arg == "--" || arg == "-r" || arg == "--resume" || arg.starts_with("--resume=")
            })
            .unwrap_or(argv.len());
        argv.splice(position..position, ["--session-dir".into(), path.into()]);
        return Ok(Some(directory.to_path_buf()));
    }
    Ok(None)
}

fn put(env: &mut BTreeMap<String, String>, key: &str, value: String) {
    env.retain(|name, _| !name.eq_ignore_ascii_case(key));
    env.insert(key.into(), value);
}

pub fn resolve(
    preset: &GjcPreset,
    runtime: &Runtime,
    router: Option<&Router>,
    launch: &Launch,
    select: &Selection,
    inputs: Inputs<'_>,
) -> Result<ResolvedGjcLaunch> {
    preset.validate()?;
    validate_runtime(preset, runtime, router)?;
    if !preset.enabled {
        return Err("The preset is disabled".into());
    }
    let mut argv: Vec<_> = runtime
        .arguments
        .iter()
        .chain(&launch.extra)
        .cloned()
        .collect();
    if argv.iter().any(|arg| !safe_argument(arg)) {
        return Err("Preset arguments contain an invalid or credential-like value".into());
    }
    if launch.resume_from.is_some() {
        if argv.iter().any(|arg| {
            matches!(
                arg.split('=').next().unwrap_or_default(),
                "--" | "-r"
                    | "--resume"
                    | "--session"
                    | "--fork"
                    | "-c"
                    | "--continue"
                    | "-p"
                    | "--print"
                    | "--mode"
                    | "--no-session"
            )
        }) {
            return Err("Remove conflicting session or mode flags before using Resume".into());
        }
        argv.push("--resume".into());
    }
    let history_mode = inputs.previous.map_or_else(
        || requested_history(preset, select),
        |snapshot| snapshot.history_mode,
    );
    let mut env = BTreeMap::new();
    let mut warnings = inputs
        .warning
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let mut models = None;
    let mut model = select
        .model
        .as_deref()
        .unwrap_or(&preset.model)
        .trim()
        .to_owned();
    let mut requires_token = false;
    let endpoint = router
        .map(|profile| endpoint(preset.protocol, profile))
        .transpose()?;
    if preset.auto_config_enabled {
        let profile = router.ok_or("Preset router is missing")?;
        if model.is_empty() || !safe_value(&model) {
            return Err("Enter a valid model ID".into());
        }
        if argv.iter().any(|arg| {
            matches!(
                arg.split('=').next().unwrap_or_default(),
                "--model" | "--provider" | "--sm" | "--mpreset" | "--models" | "--model-role"
            )
        }) {
            return Err("Remove model selection flags from runtime and session arguments. Use the preset model override.".into());
        }
        if argv.iter().any(|arg| {
            matches!(
                arg.split('=').next().unwrap_or_default(),
                "--worktree" | "--tmux" | "--session" | "--fork" | "-c" | "--continue"
            )
        }) {
            return Err("Automatic presets require a direct launch. Use the Deck Resume action to open saved conversations.".into());
        }
        let selector = format!("{}/{}", preset.provider_id, model);
        let position = argv
            .iter()
            .position(|arg| arg == "--" || arg == "--resume")
            .unwrap_or(argv.len());
        argv.splice(position..position, ["--model".into(), selector.clone()]);
        requires_token = profile.authentication == "bearer";
        models = Some(models_document(
            preset,
            endpoint.as_deref().ok_or("Router endpoint is missing")?,
            &model,
            &selector,
            requires_token,
        )?);
        if let Some(agent) = inputs
            .agent_directory
            .as_ref()
            .filter(|_| inputs.inject_agent_directory)
        {
            put(
                &mut env,
                AGENT_DIR,
                agent
                    .to_str()
                    .ok_or("Agent directory is not valid Unicode")?
                    .into(),
            );
        }
        let timeout = select.stream_timeout.as_deref().or_else(|| {
            preset
                .stream_timeout
                .enabled
                .then_some(preset.stream_timeout.value.as_str())
        });
        if let Some(timeout) = timeout {
            if timeout.parse::<u32>().ok().filter(|v| *v > 0).is_none() {
                return Err("Stream timeout must be a positive number of milliseconds".into());
            }
            if matches!(preset.protocol, Protocol::Openai) {
                put(&mut env, TIMEOUT, timeout.into());
            } else {
                warnings.push(
                    "The configured idle timeout applies to the OpenAI transport only.".into(),
                );
            }
        }
        for (key, toggle) in [
            (LANGUAGE, &preset.ui_language),
            (REGISTRY, &preset.model_preset_registry),
        ] {
            if toggle.enabled {
                if runtime.version == "0.20.0" {
                    put(&mut env, key, toggle.value.clone());
                } else {
                    warnings.push(format!("{key} has not been verified for this release."));
                }
            }
        }
        for variable in preset.variables.iter().filter(|var| var.enabled) {
            put(&mut env, &variable.key, variable.value.clone());
        }
        // Extended Windows paths are normalized by the registry before using them in argv.
        if let Some(agent) = &inputs.agent_directory {
            if !agent.is_absolute() || !inputs.app_data.is_absolute() {
                return Err("Agent directory could not be resolved".into());
            }
        }
    } else if model.is_empty() {
        model = router
            .map(|profile| profile.model.clone())
            .unwrap_or_default();
    }
    let mut keys = BTreeSet::new();
    for (key, value) in &launch.env {
        let upper = key.to_ascii_uppercase();
        if !keys.insert(upper.clone())
            || !(persistable_key(key)
                || [AGENT_DIR, TIMEOUT, LANGUAGE, REGISTRY, "NO_PROXY"].contains(&upper.as_str()))
            || !safe_value(value)
        {
            return Err("Use valid non-secret environment overrides. Enter the router credential in its password field.".into());
        }
        if upper == AGENT_DIR {
            put(
                &mut env,
                AGENT_DIR,
                inputs
                    .agent_directory
                    .as_ref()
                    .and_then(|p| p.to_str())
                    .ok_or("Invalid agent directory")?
                    .into(),
            );
        } else {
            put(&mut env, key, value.clone());
        }
    }
    put(&mut env, "GJC_HOST_STATUS", "terax".into());
    let mut binding = adapter::Binding {
        env,
        endpoint,
        model,
    };
    if let Some(profile) = router {
        adapter::merge_no_proxy(&mut binding.env, &profile.host, inputs.inherited_no_proxy);
    }
    if !preset.auto_config_enabled {
        adapter::apply_router_environment(&mut binding, &runtime.router_environment);
    }
    let explicit_history = explicit_history(&argv)?;
    let session_directory = if inputs.native_history || explicit_history.is_some() {
        // Native history preserves GJC binding validation. Explicit history preserves
        // the raw CLI argument while saving the canonical directory in the snapshot.
        inputs.session_directory
    } else {
        history_arguments(&mut argv, inputs.session_directory.as_deref())?
    };
    if launch.resume_from.is_some()
        && inputs
            .previous
            .is_some_and(|previous| previous.session_directory != session_directory)
    {
        return Err("Resume requires the previously saved session directory".into());
    }
    let models_target = models.as_ref().and_then(|_| {
        inputs
            .agent_directory
            .as_ref()
            .map(|path| path.join("models.yml"))
    });
    let history = HistorySnapshot {
        preset_id: preset.id.clone(),
        preset_revision: preset.revision,
        agent_directory: inputs.agent_directory,
        session_directory,
        history_mode,
        models_fingerprint: models.as_ref().map(|content| digest(content.as_bytes())),
        native_history: inputs.native_history,
        history_disabled: explicit_history == Some(None),
    };
    Ok(ResolvedGjcLaunch {
        history,
        runtime_id: runtime.id.clone(),
        version: runtime.version.clone(),
        executable: runtime.executable.path.clone(),
        executable_sha256: runtime.executable.sha256.clone(),
        cwd: inputs
            .cwd
            .to_str()
            .ok_or("Working directory is not valid Unicode")?
            .into(),
        router_id: router.map(|profile| profile.id.clone()),
        router_revision: router.map(|profile| profile.updated_at),
        endpoint: binding.endpoint,
        provider_id: preset.provider_id.clone(),
        protocol: preset.protocol,
        model: binding.model,
        argv,
        env: binding.env,
        models_target,
        managed_startup_config: preset.auto_config_enabled
            && preset.directory_mode == DirectoryMode::Managed
            && override_dir(launch).is_none(),
        warnings,
        requires_token,
        models,
    })
}
