use crate::{
    model::{Agent, Launch, Result, Router},
    router,
};
use std::collections::BTreeMap;

pub struct Binding {
    pub env: BTreeMap<String, String>,
    pub endpoint: Option<String>,
    pub model: String,
}

pub trait Adapter: Sync {
    fn recognizes(&self, filename: &str) -> bool;
    fn configure(&self, profile: &Router, launch: &Launch, binding: &mut Binding) -> Result<()>;
}

struct Gajae;
struct Claude;
struct Hermes;
struct Generic;

impl Adapter for Gajae {
    fn recognizes(&self, name: &str) -> bool {
        name.eq_ignore_ascii_case("gjc")
    }
    fn configure(&self, profile: &Router, launch: &Launch, binding: &mut Binding) -> Result<()> {
        manual(profile, launch, binding)
    }
}
impl Adapter for Hermes {
    fn recognizes(&self, name: &str) -> bool {
        name.eq_ignore_ascii_case("hermes")
    }
    fn configure(&self, profile: &Router, launch: &Launch, binding: &mut Binding) -> Result<()> {
        manual(profile, launch, binding)
    }
}
impl Adapter for Generic {
    fn recognizes(&self, _name: &str) -> bool {
        true
    }
    fn configure(&self, profile: &Router, launch: &Launch, binding: &mut Binding) -> Result<()> {
        manual(profile, launch, binding)
    }
}
impl Adapter for Claude {
    fn recognizes(&self, name: &str) -> bool {
        name.eq_ignore_ascii_case("claude")
    }
    fn configure(&self, profile: &Router, launch: &Launch, binding: &mut Binding) -> Result<()> {
        let path = profile
            .anthropic_path
            .as_deref()
            .ok_or("Claude Code needs an Anthropic Messages base URL")?;
        let endpoint = router::endpoint(profile, path)?;
        set(&mut binding.env, "ANTHROPIC_BASE_URL", endpoint.clone());
        if !binding.model.is_empty() {
            set(&mut binding.env, "ANTHROPIC_MODEL", binding.model.clone());
        }
        if profile.authentication == "bearer" {
            if launch.token.trim().is_empty() {
                return Err("Enter the router bearer token for this launch".into());
            }
            set(
                &mut binding.env,
                "ANTHROPIC_AUTH_TOKEN",
                launch.token.clone(),
            );
        } else {
            set(&mut binding.env, "ANTHROPIC_AUTH_TOKEN", String::new());
        }
        set(&mut binding.env, "ANTHROPIC_API_KEY", String::new());
        binding.endpoint = Some(endpoint);
        Ok(())
    }
}

fn set(env: &mut BTreeMap<String, String>, key: &str, value: String) {
    env.retain(|name, _| !name.eq_ignore_ascii_case(key));
    env.insert(key.into(), value);
}

fn manual(profile: &Router, launch: &Launch, binding: &mut Binding) -> Result<()> {
    if !launch.manual_router {
        return Err(
            "Confirm that this runtime is manually configured for the selected router".into(),
        );
    }
    let path = match launch.manual_protocol.as_str() {
        "openai" => profile.openai_path.as_ref(),
        "anthropic" => profile.anthropic_path.as_ref(),
        _ => None,
    }
    .ok_or("Select an API protocol enabled on the router")?;
    binding.endpoint = Some(router::endpoint(profile, path)?);
    Ok(())
}

pub fn for_agent(agent: Agent) -> &'static dyn Adapter {
    match agent {
        Agent::Gajae => &Gajae,
        Agent::Claude => &Claude,
        Agent::Hermes => &Hermes,
        Agent::Generic => &Generic,
    }
}

pub fn binding(agent: Agent, launch: &Launch, profile: Option<&Router>) -> Result<Binding> {
    let mut env = BTreeMap::new();
    for (key, value) in &launch.env {
        if key.is_empty() || key.contains(['=', '\0']) || value.contains('\0') {
            return Err("Invalid environment override".into());
        }
        set(&mut env, key, value.clone());
    }
    if agent == Agent::Claude {
        set(&mut env, "DISABLE_AUTOUPDATER", "1".into());
        set(
            &mut env,
            "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC",
            "1".into(),
        );
    }
    let mut result = Binding {
        env,
        endpoint: None,
        model: launch.model.trim().into(),
    };
    if let Some(profile) = profile {
        if !profile.enabled {
            return Err("Router profile is disabled".into());
        }
        router::validate(profile)?;
        if result.model.is_empty() {
            result.model = profile.model.clone();
        }
        for_agent(agent).configure(profile, launch, &mut result)?;
        merge_no_proxy(&mut result.env, &profile.host, inherited_no_proxy());
    }
    Ok(result)
}

pub fn validate_router_environment(bindings: &BTreeMap<String, String>) -> Result<()> {
    for (key, source) in bindings {
        if key.is_empty()
            || !key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
            || matches!(key.to_ascii_uppercase().as_str(), "PATH" | "NO_PROXY")
            || !matches!(source.as_str(), "endpoint" | "model")
        {
            return Err("Router environment bindings require NAME=endpoint or NAME=model".into());
        }
    }
    Ok(())
}

pub fn apply_router_environment(binding: &mut Binding, variables: &BTreeMap<String, String>) {
    if let Some(endpoint) = &binding.endpoint {
        for (key, source) in variables {
            let value = if source == "endpoint" {
                endpoint
            } else {
                &binding.model
            };
            set(&mut binding.env, key, value.clone());
        }
    }
}

pub fn inherited_no_proxy() -> [Option<String>; 2] {
    [
        std::env::var("NO_PROXY").ok(),
        std::env::var("no_proxy").ok(),
    ]
}

pub fn merge_no_proxy(
    env: &mut BTreeMap<String, String>,
    host: &str,
    inherited: [Option<String>; 2],
) {
    let host = host.trim_matches(['[', ']']);
    let mut no_proxy = Vec::new();
    for value in [
        inherited[0].clone(),
        inherited[1].clone(),
        env.iter()
            .find(|(key, _)| key.eq_ignore_ascii_case("NO_PROXY"))
            .map(|(_, value)| value.clone()),
        Some(host.into()),
    ]
    .into_iter()
    .flatten()
    {
        for part in value
            .split(',')
            .map(str::trim)
            .filter(|part| !part.is_empty())
        {
            if !no_proxy
                .iter()
                .any(|entry: &String| entry.eq_ignore_ascii_case(part))
            {
                no_proxy.push(part.to_owned());
            }
        }
    }
    let value = no_proxy.join(",");
    set(env, "NO_PROXY", value.clone());
    #[cfg(not(windows))]
    env.insert("no_proxy".into(), value);
}
