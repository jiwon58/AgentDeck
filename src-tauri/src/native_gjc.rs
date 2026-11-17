use crate::{
    model::{error, NativeGjcSnapshot, Result, Router},
    registry, router,
};
use serde::{Deserialize, Serialize};
use serde_yaml_ng::{Mapping, Value};
use std::{
    fs::{self, File},
    io::{Read, Write},
    path::Path,
};

pub const TOKEN: &str = "GJC_DECK_ROUTER_TOKEN";
pub const PRESETS: &str = "native_gjc_presets";

#[derive(Deserialize)]
pub struct Selection {
    pub preset: Preset,
    pub catalog: Vec<String>,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum HistoryLocation {
    #[default]
    AppData,
    Custom,
    Workspace,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Preset {
    pub name: String,
    pub runtime_id: String,
    pub router_id: String,
    pub anthropic: bool,
    pub models_template: String,
    pub config_template: String,
    pub provider: String,
    pub default_model: String,
    pub models: Vec<String>,
    pub context: u32,
    pub max_output: u32,
    pub location: HistoryLocation,
    pub history_root: String,
}

impl Default for Preset {
    fn default() -> Self {
        Self {
            name: String::new(),
            runtime_id: String::new(),
            router_id: String::new(),
            anthropic: false,
            models_template: String::new(),
            config_template: String::new(),
            provider: "deck-router".into(),
            default_model: String::new(),
            models: vec![],
            context: 0,
            max_output: 0,
            location: HistoryLocation::default(),
            history_root: String::new(),
        }
    }
}

pub fn protocol(preset: &Preset) -> router::ModelProtocol {
    if preset.anthropic {
        router::ModelProtocol::Anthropic
    } else {
        router::ModelProtocol::Openai
    }
}

pub struct Documents {
    pub models: String,
    pub config: String,
    pub endpoint: String,
    pub selector: String,
}

pub fn refresh_documents(
    preset: &Preset,
    profile: &Router,
    token: &str,
    expected: &[String],
) -> Result<Documents> {
    if !profile.enabled || (profile.authentication == "bearer" && token.is_empty()) {
        return Err("Enable the Router and enter its credential".into());
    }
    let refreshed = router::list_models(profile, token, protocol(preset))?;
    if !refreshed.complete {
        return Err(refreshed.message.into());
    }
    if refreshed.models != expected {
        return Err(
            "The Router catalog changed. Refresh models and review the selection before launching."
                .into(),
        );
    }
    documents(preset, profile, &refreshed)
}

pub fn configure_command(
    command: &mut portable_pty::CommandBuilder,
    paths: &NativeGjcSnapshot,
    version: &str,
    token: &str,
    endpoint: Option<&str>,
) {
    command.env("GJC_CODING_AGENT_DIR", &paths.agent_directory);
    command.env_remove("PI_CODING_AGENT_DIR");
    command.env_remove("GJC_AGENT_DIR");
    command.env_remove(TOKEN);
    if version == "0.20.0" {
        command.env("GJC_MODEL_PRESET_REGISTRY_DISABLED", "1");
    }
    if !token.is_empty() {
        command.env(TOKEN, token);
    }
    if let Some(host) = endpoint
        .and_then(|uri| reqwest::Url::parse(uri).ok())
        .and_then(|uri| uri.host_str().map(str::to_owned))
        .filter(|host| router::loopback(host))
    {
        let inherited = std::env::var("NO_PROXY")
            .or_else(|_| std::env::var("no_proxy"))
            .unwrap_or_default();
        let value = if inherited.is_empty() {
            host
        } else {
            format!("{inherited},{host}")
        };
        command.env("NO_PROXY", value);
    }
}

fn mapping(value: &mut Value) -> Result<&mut Mapping> {
    value
        .as_mapping_mut()
        .ok_or_else(|| "Expected a YAML mapping".into())
}

fn section<'a>(parent: &'a mut Mapping, key: &str) -> Result<&'a mut Mapping> {
    mapping(
        parent
            .entry(key.into())
            .or_insert_with(|| Value::Mapping(Mapping::new())),
    )
}

fn reject_inline_secrets(value: &Value) -> Result<()> {
    match value {
        Value::Mapping(map) => {
            for (key, value) in map {
                if key.as_str().is_some_and(|name| {
                    matches!(
                        name.to_ascii_lowercase().as_str(),
                        "apikey"
                            | "api_key"
                            | "authtoken"
                            | "accesstoken"
                            | "refreshtoken"
                            | "password"
                            | "authorization"
                    )
                }) && !value.is_null()
                {
                    return Err("Use environment references in YAML templates instead of inline credentials".into());
                }
                reject_inline_secrets(value)?;
            }
        }
        Value::Sequence(items) => {
            for item in items {
                reject_inline_secrets(item)?;
            }
        }
        Value::Tagged(_) => {
            return Err("YAML templates must use standard mappings and values".into())
        }
        _ => {}
    }
    Ok(())
}

fn template(path: &str) -> Result<Value> {
    if path.trim().is_empty() {
        return Ok(Value::Mapping(Mapping::new()));
    }
    let path = Path::new(path);
    if !path.is_absolute()
        || !matches!(
            path.extension().and_then(|ext| ext.to_str()),
            Some("yml" | "yaml")
        )
    {
        return Err("Use an absolute path to a .yml or .yaml template".into());
    }
    let file = File::open(path).map_err(error)?;
    let metadata = file.metadata().map_err(error)?;
    if !metadata.is_file() || metadata.len() > 1024 * 1024 {
        return Err("YAML templates must be regular files up to 1 MiB".into());
    }
    let mut text = String::new();
    file.take(1024 * 1024 + 1)
        .read_to_string(&mut text)
        .map_err(error)?;
    if text.len() > 1024 * 1024 {
        return Err("YAML template exceeds 1 MiB".into());
    }
    let mut value: Value = serde_yaml_ng::from_str(&text)
        .map_err(|err| format!("Invalid YAML template at {:?}", err.location()))?;
    if value.is_null() {
        value = Value::Mapping(Mapping::new());
    }
    reject_inline_secrets(&value)?;
    Ok(value)
}

pub fn documents(
    preset: &Preset,
    profile: &Router,
    catalog: &router::ModelCatalog,
) -> Result<Documents> {
    let configured_path = if preset.anthropic {
        &profile.anthropic_path
    } else {
        &profile.openai_path
    };
    let endpoint = router::endpoint(
        profile,
        configured_path
            .as_deref()
            .ok_or("Enable the selected Router protocol")?,
    )?;
    if preset.provider.is_empty()
        || !preset
            .provider
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_".contains(&byte))
    {
        return Err("Use letters, digits, hyphens or underscores in the provider ID".into());
    }
    let selected: Vec<_> = catalog
        .models
        .iter()
        .filter(|id| preset.models.is_empty() || preset.models.contains(id))
        .collect();
    if selected.is_empty()
        || !selected
            .iter()
            .any(|id| id.as_str() == preset.default_model)
    {
        return Err("Refresh the Router models and choose a currently served default model".into());
    }
    if !preset.models.iter().all(|id| catalog.models.contains(id)) {
        return Err(
            "A selected model is no longer served. Refresh the Router model selection.".into(),
        );
    }
    let mut models = template(&preset.models_template)?;
    let providers = section(mapping(&mut models)?, "providers")?;
    let provider = section(providers, &preset.provider)?;
    let old_models = provider
        .get("models")
        .and_then(Value::as_sequence)
        .cloned()
        .unwrap_or_default();
    let mut generated = Vec::new();
    for id in selected {
        let mut model = old_models
            .iter()
            .find(|model| model["id"].as_str() == Some(id.as_str()))
            .cloned()
            .unwrap_or_else(|| Value::Mapping(Mapping::new()));
        let fields = mapping(&mut model)?;
        let limits = catalog.limits.get(id);
        let context = limits
            .and_then(|limits| limits.context)
            .map(u64::from)
            .or_else(|| fields.get("contextWindow").and_then(Value::as_u64))
            .unwrap_or(preset.context.into());
        let output = limits
            .and_then(|limits| limits.output)
            .map(u64::from)
            .or_else(|| fields.get("maxTokens").and_then(Value::as_u64))
            .unwrap_or(preset.max_output.into());
        if context == 0 || output == 0 || output > context || context > u32::MAX.into() {
            return Err(format!(
                "Provide valid context and output token limits for {id}"
            ));
        }
        fields.insert("id".into(), id.as_str().into());
        fields
            .entry("name".into())
            .or_insert_with(|| id.as_str().into());
        fields.insert("contextWindow".into(), context.into());
        fields.insert("maxTokens".into(), output.into());
        generated.push(model);
    }
    provider.insert("baseUrl".into(), endpoint.as_str().into());
    provider.insert(
        "api".into(),
        if preset.anthropic {
            "anthropic-messages"
        } else {
            "openai-completions"
        }
        .into(),
    );
    provider.insert("apiKeyEnv".into(), TOKEN.into());
    provider.insert(
        "auth".into(),
        if profile.authentication == "bearer" {
            "apiKey"
        } else {
            "none"
        }
        .into(),
    );
    provider.insert("models".into(), Value::Sequence(generated));
    let selector = format!("{}/{}", preset.provider, preset.default_model);
    let bindings = section(mapping(&mut models)?, "modelBindings")?;
    section(bindings, "modelRoles")?.insert("default".into(), selector.as_str().into());

    let mut config = template(&preset.config_template)?;
    let config_map = mapping(&mut config)?;
    section(config_map, "startup")?.insert("checkUpdate".into(), false.into());
    section(config_map, "modelRoles")?.insert("default".into(), selector.as_str().into());
    Ok(Documents {
        models: serde_yaml_ng::to_string(&models).map_err(error)?,
        config: serde_yaml_ng::to_string(&config).map_err(error)?,
        endpoint,
        selector,
    })
}

pub fn prepare(
    app_data: &Path,
    cwd: &Path,
    id: uuid::Uuid,
    preset: &Preset,
    documents: &Documents,
) -> Result<NativeGjcSnapshot> {
    let root = app_data.join("gjc/native");
    fs::create_dir_all(&root).map_err(error)?;
    let generated = tempfile::Builder::new()
        .prefix("session-")
        .tempdir_in(root)
        .map_err(error)?;
    for (name, text) in [
        ("models.yml", &documents.models),
        ("config.yml", &documents.config),
    ] {
        let mut file = File::create_new(generated.path().join(name)).map_err(error)?;
        file.write_all(text.as_bytes()).map_err(error)?;
    }
    let session_directory = match preset.location {
        HistoryLocation::AppData => generated.path().join("sessions"),
        HistoryLocation::Custom => registry::directory(&preset.history_root)?.join(id.to_string()),
        HistoryLocation::Workspace => {
            let parent = cwd.join(".gjc").join("deck-sessions");
            for directory in [cwd.join(".gjc"), parent.clone()] {
                if directory.exists()
                    && !dunce::canonicalize(&directory)
                        .map_err(error)?
                        .starts_with(cwd)
                {
                    return Err(
                        "The workspace history folder resolves outside the working directory"
                            .into(),
                    );
                }
            }
            parent.join(id.to_string())
        }
    };
    if session_directory.exists() {
        return Err("The new GJC session history path already exists".into());
    }
    fs::create_dir_all(&session_directory).map_err(error)?;
    Ok(NativeGjcSnapshot {
        agent_directory: generated.keep(),
        session_directory,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn router_refresh_only_updates_the_generated_pair_and_isolates_history() {
        let temp = tempfile::tempdir().unwrap();
        let cwd = dunce::canonicalize(temp.path()).unwrap();
        let models_path = cwd.join("original-models.yml");
        let config_path = cwd.join("original-config.yml");
        let models_template = "providers:\n  deck-router:\n    models:\n      - id: alpha\n        reasoning: true\n        maxTokens: 8192\n      - id: removed\n  untouched:\n    api: openai-completions\n";
        let config_template =
            "startup:\n  checkUpdate: true\nmodelRoles:\n  smol: elsewhere/fast\n";
        fs::write(&models_path, models_template).unwrap();
        fs::write(&config_path, config_template).unwrap();
        let profile = Router {
            id: "router".into(),
            name: "Fixture".into(),
            scheme: "http".into(),
            host: "127.0.0.1".into(),
            port: 12345,
            openai_path: Some("/v1".into()),
            anthropic_path: None,
            model: String::new(),
            authentication: "bearer".into(),
            enabled: true,
            allow_remote: false,
            created_at: 0,
            updated_at: 0,
        };
        let mut preset = Preset {
            models_template: models_path.display().to_string(),
            config_template: config_path.display().to_string(),
            default_model: "alpha".into(),
            context: 32768,
            max_output: 4096,
            ..Preset::default()
        };
        let mut catalog = router::ModelCatalog {
            models: vec!["alpha".into(), "beta".into()],
            complete: true,
            limits: [(
                "alpha".into(),
                router::ModelLimits {
                    context: Some(65536),
                    output: None,
                },
            )]
            .into(),
            http: Some(200),
            message: "",
        };
        let first = documents(&preset, &profile, &catalog).unwrap();
        let models: Value = serde_yaml_ng::from_str(&first.models).unwrap();
        let provider = &models["providers"]["deck-router"];
        assert_eq!(provider["models"][0]["contextWindow"].as_u64(), Some(65536));
        assert_eq!(provider["models"][0]["maxTokens"].as_u64(), Some(8192));
        assert_eq!(provider["models"][0]["reasoning"].as_bool(), Some(true));
        assert_eq!(provider["models"][1]["id"].as_str(), Some("beta"));
        assert_eq!(provider["apiKeyEnv"].as_str(), Some(TOKEN));
        assert!(models["providers"].get("untouched").is_some());
        let config: Value = serde_yaml_ng::from_str(&first.config).unwrap();
        assert_eq!(
            config["modelRoles"]["default"].as_str(),
            Some("deck-router/alpha")
        );
        assert_eq!(
            config["modelRoles"]["smol"].as_str(),
            Some("elsewhere/fast")
        );
        assert_eq!(config["startup"]["checkUpdate"].as_bool(), Some(false));
        let app = prepare(&cwd, &cwd, uuid::Uuid::new_v4(), &preset, &first).unwrap();
        assert!(app.session_directory.starts_with(&app.agent_directory));
        catalog.models.remove(0);
        assert!(documents(&preset, &profile, &catalog).is_err());
        preset.default_model = "beta".into();
        let second = documents(&preset, &profile, &catalog).unwrap();
        assert!(!second.models.contains("alpha"));
        preset.location = HistoryLocation::Workspace;
        let work = prepare(&cwd, &cwd, uuid::Uuid::new_v4(), &preset, &second).unwrap();
        assert!(work
            .session_directory
            .starts_with(cwd.join(".gjc/deck-sessions")));
        preset.location = HistoryLocation::Custom;
        preset.history_root = cwd.display().to_string();
        let custom = prepare(&cwd, &cwd, uuid::Uuid::new_v4(), &preset, &second).unwrap();
        assert_eq!(custom.session_directory.parent(), Some(cwd.as_path()));
        assert_ne!(work.agent_directory, custom.agent_directory);
        assert_eq!(
            fs::read_to_string(app.agent_directory.join("models.yml")).unwrap(),
            first.models
        );
        assert_eq!(fs::read_to_string(models_path).unwrap(), models_template);
        assert_eq!(fs::read_to_string(config_path).unwrap(), config_template);
    }
}
