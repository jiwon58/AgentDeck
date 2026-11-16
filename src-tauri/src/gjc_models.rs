use super::{digest, GjcPreset, Protocol, ResolvedGjcLaunch, KEY};
use crate::model::{error, Result};
use serde_json::{json, Value};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::Path,
};

const MARKER: &str = "# Managed by Agent Deck sha256:";
const MAX_CONFIG_BYTES: u64 = 1024 * 1024;

pub(super) fn models_document(
    preset: &GjcPreset,
    endpoint: &str,
    model: &str,
    selector: &str,
    auth: bool,
) -> Result<String> {
    let api = match preset.protocol {
        Protocol::Openai => "openai-completions",
        Protocol::Anthropic => "anthropic-messages",
    };
    // JSON flow syntax is valid YAML; the structured serializer escapes all user input.
    let document = json!({
        "providers": { (&preset.provider_id): {
            "baseUrl": endpoint, "api": api, "apiKeyEnv": KEY, "auth": if auth {"apiKey"} else {"none"},
            "models": [{
                "id": model, "name": model, "reasoning": false, "input": ["text"],
                "cost": {"input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0},
                "contextWindow": preset.context_tokens, "maxTokens": preset.max_output_tokens,
            }]
        }},
        "modelBindings": { "modelRoles": { "default": selector }}
    });
    let body = format!(
        "{}\n",
        serde_json::to_string_pretty(&document).map_err(error)?
    );
    Ok(format!("{MARKER}{}\n{body}", digest(body.as_bytes())))
}

fn open_lock(directory: &Path, name: &str) -> Result<File> {
    let path = directory.join(name);
    if path
        .symlink_metadata()
        .is_ok_and(|metadata| !metadata.file_type().is_file())
    {
        return Err("Agent directory lock is not a regular file".into());
    }
    OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
        .map_err(error)
}

#[derive(Default)]
pub struct Prepared {
    pub lease: Option<File>,
    pub writer: Option<File>,
}

pub fn validate_targets(plan: &ResolvedGjcLaunch) -> Result<()> {
    if let Some(target) = &plan.models_target {
        if let Some(current) = read_models(target)? {
            if plan.models.as_deref() != Some(&current) && !is_deck_document(&current) {
                return Err("This models.yml contains user configuration. Select a managed directory or disable automatic configuration.".into());
            }
            if target.metadata().map_err(error)?.permissions().readonly() {
                return Err("models.yml is read-only".into());
            }
        }
        let parent = target
            .ancestors()
            .skip(1)
            .find(|path| path.exists())
            .ok_or("Agent directory has no existing parent")?;
        let metadata = parent.metadata().map_err(error)?;
        if !metadata.is_dir()
            || metadata.permissions().readonly()
            || dunce::canonicalize(parent).map_err(error)? != parent
        {
            return Err("The agent directory is not writable".into());
        }
    }
    Ok(())
}

fn is_deck_document(content: &str) -> bool {
    content
        .strip_prefix(MARKER)
        .and_then(|rest| rest.split_once('\n'))
        .filter(|(fingerprint, body)| *fingerprint == digest(body.as_bytes()))
        .and_then(|(_, body)| serde_json::from_str::<Value>(body).ok())
        .is_some_and(|document| deck_owned(&document))
}

pub fn prepare(plan: &ResolvedGjcLaunch) -> Result<Prepared> {
    validate_targets(plan)?;
    let Some((target, desired)) = plan.models_target.as_ref().zip(plan.models.as_ref()) else {
        let lease = plan
            .history
            .agent_directory
            .as_deref()
            .map(|directory| {
                let writer = open_lock(directory, ".agent-deck-models-write.lock")?;
                writer.lock().map_err(error)?;
                let lease = open_lock(directory, ".agent-deck-models-live.lock")?;
                lease.lock_shared().map_err(error)?;
                Ok::<_, String>((lease, writer))
            })
            .transpose()?;
        return Ok(match lease {
            Some((lease, writer)) => Prepared {
                lease: Some(lease),
                writer: Some(writer),
            },
            None => Prepared {
                lease: None,
                writer: None,
            },
        });
    };
    let directory = target.parent().ok_or("Invalid models.yml location")?;
    fs::create_dir_all(directory).map_err(error)?;
    if dunce::canonicalize(directory).map_err(error)? != directory {
        return Err("The agent directory resolves to a different path; review its location".into());
    }
    let writer = open_lock(directory, ".agent-deck-models-write.lock")?;
    writer.lock().map_err(error)?;
    let lease = open_lock(directory, ".agent-deck-models-live.lock")?;
    let current = read_models(target)?;
    if current.as_deref() != Some(desired) {
        lease.try_lock().map_err(|_| {
            crate::diagnostics::warn("gjc.models_live_conflict", Some(&plan.history.preset_id));
            "A live session uses this models.yml. Close it or duplicate the preset before changing the model configuration."
        })?;
        if let Some(content) = current {
            if !is_deck_document(&content) {
                crate::diagnostics::warn(
                    "gjc.models_user_configuration",
                    Some(&plan.history.preset_id),
                );
                return Err("This models.yml contains user configuration. Select a managed directory or disable automatic configuration.".into());
            }
            let backup = directory.join(format!(
                "models.yml.bak.{}.{}",
                crate::model::now(),
                crate::model::id()
            ));
            write_models(&backup, &content)?;
        } else if plan.managed_startup_config {
            seed_managed_settings(directory)?;
        }
        write_models(target, desired)?;
        crate::diagnostics::info("gjc.models_prepared", Some(&plan.history.preset_id));
        lease.unlock().map_err(error)?;
    }
    lease.lock_shared().map_err(error)?;
    Ok(Prepared {
        lease: Some(lease),
        writer: Some(writer),
    })
}

fn seed_managed_settings(directory: &Path) -> Result<()> {
    let path = directory.join("config.yml");
    match path.symlink_metadata() {
        Ok(_) => return Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => return Err(error(err)),
    }
    let mut temp = tempfile::NamedTempFile::new_in(directory).map_err(error)?;
    temp.write_all(b"startup:\n  checkUpdate: false\n")
        .map_err(error)?;
    temp.as_file().sync_all().map_err(error)?;
    match temp.persist_noclobber(path) {
        Ok(_) => Ok(()),
        Err(err) if err.error.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
        Err(err) => Err(error(err)),
    }
}

pub(super) fn read_models(path: &Path) -> Result<Option<String>> {
    match path.symlink_metadata() {
        Ok(metadata) if metadata.file_type().is_file() && metadata.len() <= MAX_CONFIG_BYTES => {
            let mut content = String::new();
            File::open(path)
                .map_err(error)?
                .take(MAX_CONFIG_BYTES + 1)
                .read_to_string(&mut content)
                .map_err(error)?;
            if content.len() as u64 > MAX_CONFIG_BYTES {
                return Err("models.yml is too large".into());
            }
            Ok(Some(content))
        }
        Ok(_) => Err("models.yml must be a regular file no larger than 1 MiB".into()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(error(err)),
    }
}

fn deck_owned(document: &Value) -> bool {
    let Some(root) = document.as_object() else {
        return false;
    };
    let Some(providers) = document["providers"].as_object() else {
        return false;
    };
    root.len() == 2
        && providers.len() == 1
        && providers.values().all(|provider| {
            provider["apiKeyEnv"] == KEY
                && provider.as_object().is_some_and(|fields| {
                    fields.keys().all(|key| {
                        matches!(
                            key.as_str(),
                            "api" | "apiKeyEnv" | "auth" | "baseUrl" | "models"
                        )
                    })
                })
        })
}

fn write_models(path: &Path, content: &str) -> Result<()> {
    let parent = path.parent().ok_or("Invalid models.yml path")?;
    let mut temp = tempfile::NamedTempFile::new_in(parent).map_err(error)?;
    temp.write_all(content.as_bytes()).map_err(error)?;
    temp.flush().map_err(error)?;
    temp.as_file().sync_all().map_err(error)?;
    let verified = read_models(temp.path())?.ok_or("Could not read temporary models.yml")?;
    if !is_deck_document(&verified) {
        return Err("Invalid models.yml provider document".into());
    }
    temp.persist(path).map_err(error)?;
    Ok(())
}
